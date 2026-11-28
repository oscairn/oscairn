// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Moving library items to a newer installed version of a plugin (RFC 0043 step 2). A job off the
// control thread opens each node that uses the plugin at the old version with the new version's
// build (loadPlugin offline: a worker of its own where workers are used, with the worker's memory
// limit and reply timeout), restores the node's saved state, compares its ports with the
// document's and its parameters with the old build's, and for a move saves the state again as the
// new version writes it (decided by the owner 2026-11-28: the kept-warm take matches a state
// against its file, so the file must hold the new version's state). The job works on a copy of
// each item; the control thread swaps the copy in whole and keeps the item as it was under
// Moves/<move>/ in the data folder, so nothing is lost and undo is putting the folder back.
#include "oscairn/session.h"
#include "oscairn/assets.h"
#include "oscairn/library.h"
#include "oscairn/package.h"
#include "oscairn/setlist.h"
#include "oscairn/signing.h"
#include "../../engine/src/PreparedInstrument.h"
#include "../../plugin-host/src/StateFiles.h"
#include "JsonInput.h"
#include "sheredom/json.h"
#include <algorithm>
#include <chrono>
#include <cmath>
#include <ctime>
#include <random>
#include <thread>

namespace oscairn
{
    namespace
    {
        namespace fs = std::filesystem;
        using V = const json_value_s*;
        using Doc = std::unique_ptr<json_value_s, decltype(&std::free)>;
        constexpr size_t mostItems = 10000; // a plan's items at most (RFC 0043's security table)

        std::string text(V v) { return v && v->type == json_type_string ? std::string(static_cast<const json_string_s*>(v->payload)->string, static_cast<const json_string_s*>(v->payload)->string_size) : std::string(); }
        std::vector<V> elements(V v)
        {
            std::vector<V> out;
            if(v && v->type == json_type_array) for(auto* e = static_cast<const json_array_s*>(v->payload)->start; e; e = e->next) out.push_back(e->value);
            return out;
        }
        uint32_t whole(V v) { return v && v->type == json_type_number ? uint32_t(std::strtoul(static_cast<const json_number_s*>(v->payload)->number, nullptr, 10)) : 0; }
        std::string slurp(const fs::path& file) { const auto bytes = state_files::read(file, 4 * 1024 * 1024); return {bytes.begin(), bytes.end()}; }
        Doc parse(const std::string& s)
        {
            if(!detail::jsonPreflight(s, 32).empty()) throw std::runtime_error("its document is not valid JSON");
            Doc d{json_parse(s.data(), s.size()), &std::free};
            if(!d) throw std::runtime_error("its document is not valid JSON");
            return d;
        }
        // A document written again as it was read, but for the values given by node.
        void write(std::string& out, V v, const std::map<V, std::string>& with)
        {
            if(const auto w = with.find(v); w != with.end()) { out += w->second; return; }
            switch(v->type)
            {
            case json_type_string: out += detail::quote(text(v)); break;
            case json_type_number: out.append(static_cast<const json_number_s*>(v->payload)->number, static_cast<const json_number_s*>(v->payload)->number_size); break;
            case json_type_object:
                out += '{';
                for(auto* e = static_cast<const json_object_s*>(v->payload)->start; e; e = e->next)
                {
                    out += detail::quote(std::string_view(e->name->string, e->name->string_size)) + ':';
                    write(out, e->value, with);
                    if(e->next) out += ',';
                }
                out += '}';
                break;
            case json_type_array:
                out += '[';
                for(auto* e = static_cast<const json_array_s*>(v->payload)->start; e; e = e->next) { write(out, e->value, with); if(e->next) out += ','; }
                out += ']';
                break;
            case json_type_true: out += "true"; break;
            case json_type_false: out += "false"; break;
            default: out += "null";
            }
        }
        std::string fresh() { std::random_device random; constexpr char hex[] = "0123456789abcdef"; std::string id; for(int i = 0; i < 16; ++i) id += hex[random() & 15]; return id; }

        // The plugin store, as performance.cpp keeps it: Plugins/<format>/<id>@<version>/<platform>/<bundle>.
        fs::path storeFolder(const std::string& format, const std::string& id, const std::string& version) { return dataHome() / "Plugins" / format / pluginFolderName(id, version); }
        std::map<std::string, fs::path> storedBuilds(const std::string& format, const std::string& id, const std::string& version)
        {
            std::map<std::string, fs::path> out;
            std::error_code e;
            for(const auto& p : fs::directory_iterator(storeFolder(format, id, version), e))
            {
                if(!p.is_directory() || !knownPlatform(p.path().filename().string())) continue;
                for(const auto& b : fs::directory_iterator(p.path(), e))
                    if(b.path().extension() == (format == "lv2" ? ".lv2" : ".clap")) { out[p.path().filename().string()] = b.path(); break; }
            }
            return out;
        }
        // Whether `key` signed this build of the plugin: a signature in <build>.sig over its message.
        bool signedBy(const std::string& key, const fs::path& build, const std::string& format, const std::string& id, const std::string& version, const std::string& platform)
        {
            try
            {
                const auto message = signing::buildMessage(format, id, version, platform, bundleDigest(build));
                for(const auto& s : signing::readSignatures(build.string() + ".sig")) if(s.key == key && signing::verify(s, message)) return true;
            }
            catch(const std::exception&) {}
            return false;
        }
        // The publisher's claim kept with `to` in the store (RFC 0043), honored only for a move from a
        // version it covers, both dotted numbers, by the key that signed `to`'s build here and an
        // installed build of `from`: the same publisher.
        std::optional<Control::VersionClaim> claimFor(const Control::PluginVersion& p, const std::string& to)
        {
            Control::VersionClaim c;
            try
            {
                const auto doc = parse(slurp(storeFolder(p.format, p.id, to) / "plugin.json"));
                c.opensStateFrom = text(detail::member(doc.get(), "opensStateFrom"));
                c.soundChanges = text(detail::member(doc.get(), "soundChanges"));
                c.key = text(detail::member(doc.get(), "claimKey"));
            }
            catch(const std::exception&) { return std::nullopt; }
            if(c.opensStateFrom.empty() || c.key.empty() || !dottedVersion(p.version) || !dottedVersion(to) || assets::older(p.version, c.opensStateFrom)) return std::nullopt;
            const auto here = storedBuilds(p.format, p.id, to);
            if(!here.count(platform()) || !signedBy(c.key, here.at(platform()), p.format, p.id, to, platform())) return std::nullopt;
            for(const auto& [forPlatform, build] : storedBuilds(p.format, p.id, p.version))
                if(signedBy(c.key, build, p.format, p.id, p.version, forPlatform)) return c;
            return std::nullopt; // shortcut: the old version's build must be here; an index listing it under the same key (RFC 0043) is not read yet
        }
        std::string date()
        {
            const auto now = std::chrono::system_clock::to_time_t(std::chrono::system_clock::now());
            std::tm utc{}; gmtime_r(&now, &utc);
            char out[32]; std::strftime(out, sizeof out, "%Y-%m-%dT%H:%M:%SZ", &utc);
            return out;
        }
        fs::path movesFolder() { return dataHome() / "Moves"; }

        // What the job needs, read on the control thread before it starts.
        struct Target
        {
            Control::PluginVersion plugin; std::string to;
            fs::path build, old;                // the build of `to` here, and of `from` when it is here (else empty)
            std::vector<std::string> lock;      // `to`'s lock entries: each platform's build in the store, by digest
            bool claimed = false, move = false;
            double rate = 48000; uint32_t frames = 512;
            fs::path assets;                    // the asset store's root, for plugins whose state names samples
        };
        // One item checked, and for a move its rewritten copy: the revision it was read at and the one written.
        struct Checked { Control::VersionItem item; fs::path copy; std::string was, wrote; };

        std::shared_ptr<PluginHost> open(const fs::path& build, const std::string& id, const std::string& version, const Target& t, assets::Store& assets)
        {
            auto host = loadPlugin(build.string(), id, t.rate, t.frames, ProcessingMode::Offline);
            if(host->descriptor().version != version) throw std::runtime_error("the build here is version " + host->descriptor().version + ", not " + version);
            host->setAssets(&assets);
            // As the graph holds it, so its snapshot is the one the graph saved (an instrument's pool of prepared sounds).
            host = preparedInstrument(std::move(host), t.rate, t.frames, ProcessingMode::Offline, {});
            host->setAssets(&assets);
            return host;
        }
        bool samePorts(V list, const std::vector<kick::clap::Port>& ports)
        {
            const auto saved = elements(list);
            if(saved.size() != ports.size()) return false;
            for(size_t i = 0; i < ports.size(); ++i)
                if(whole(detail::member(saved[i], "id")) != ports[i].id || whole(detail::member(saved[i], "channels")) != ports[i].channels || whole(detail::member(saved[i], "flags")) != ports[i].flags) return false;
            return true;
        }
        std::map<clap_id, std::pair<std::string, double>> values(const PluginHost& h)
        {
            std::map<clap_id, std::pair<std::string, double>> out;
            for(const auto& p : h.parameters()) out[p.id] = {p.name, h.parameterValue(p.id)};
            return out;
        }

        // An item opened with the new version, node by node; for a move, its copy rewritten. Returns
        // nothing for an item that does not use the plugin at the old version.
        std::optional<Checked> check(const Control::LibraryItem& item, const Target& t, const fs::path& copy, assets::Store& assets)
        {
            Checked out;
            auto& r = out.item;
            r.id = item.id; r.name = item.name; r.kind = item.kind;
            const auto type = itemType(item.kind);
            if(!type || type->document.empty()) return std::nullopt;
            const fs::path folder = item.path;
            std::string source;
            try { source = slurp(folder / type->document); } catch(const std::exception&) { return std::nullopt; }
            const auto doc = parse(source);
            std::vector<V> using_;
            for(const auto node : elements(detail::member(doc.get(), "nodes")))
            {
                const V plugin = detail::member(node, "plugin");
                if(text(detail::member(plugin, "format")) == t.plugin.format && text(detail::member(plugin, "id")) == t.plugin.id && text(detail::member(plugin, "version")) == t.plugin.version) using_.push_back(node);
            }
            if(using_.empty()) return std::nullopt;
            r.nodes = uint32_t(using_.size());
            r.locked = !item.library.empty() || item.readOnly || item.inPlace;
            if(r.locked) { r.status = "locked"; return out; } // the publisher moves a locked library's items, with its next version (RFC 0043)
            try
            {
                out.was = readInfo(std::string_view(source), type->format).revision;
                const auto base = t.move ? copy : folder;
                if(t.move) fs::copy(folder, copy, fs::copy_options::recursive);
                std::map<V, std::string> with;
                for(const auto node : using_)
                {
                    const auto key = text(detail::member(node, "key"));
                    if(key.empty() || key.find('/') != std::string::npos || key.front() == '.') throw std::runtime_error("a node's key is not a plain name");
                    const V sound = detail::member(node, "sound");
                    const bool shared = sound && sound->type != json_type_null;
                    // A node sharing an instance saves its values only; the instance's state is with the first node of its engine.
                    const auto dir = !shared ? base / "nodes" / key : text(detail::member(node, "engine")) == key ? base / "nodes" / key / "engine" : fs::path();
                    const auto name = text(detail::member(node, "name"));
                    if(!dir.empty())
                    {
                        auto opened = open(t.build, t.plugin.id, t.to, t, assets);
                        if(!shared && (!samePorts(detail::member(node, "inputs"), opened->inputPorts()) || !samePorts(detail::member(node, "outputs"), opened->outputPorts()) ||
                                       whole(detail::member(node, "noteInputs")) != opened->noteInputPorts() || whole(detail::member(node, "noteOutputs")) != opened->noteOutputPorts()))
                            throw std::runtime_error(name + ": its audio or note ports are not the same in " + t.to);
                        try { opened->loadSnapshot(dir.string()); }
                        catch(const std::exception& e) { throw std::runtime_error(name + ": its saved state does not open in " + t.to + " (" + e.what() + ")"); }
                        const auto after = values(*opened);
                        // Compared with what the old version reads from the same state, when its build is here.
                        if(!t.old.empty())
                        {
                            auto older = open(t.old, t.plugin.id, t.plugin.version, t, assets);
                            try { older->loadSnapshot(dir.string()); }
                            catch(const std::exception& e) { throw std::runtime_error(name + ": " + t.plugin.version + " itself does not open its saved state (" + e.what() + ")"); }
                            const auto before = values(*older);
                            older.reset();
                            for(const auto& [id, v] : before)
                            {
                                const auto a = after.find(id);
                                if(a == after.end()) r.changes.push_back({key, v.first, id, v.second, std::nullopt});
                                else if(std::abs(a->second.second - v.second) > 1e-9) r.changes.push_back({key, v.first, id, v.second, a->second.second});
                            }
                            for(const auto& [id, v] : after) if(!before.count(id)) r.changes.push_back({key, v.first, id, std::nullopt, v.second});
                        }
                        if(t.move)
                        {
                            // The state as the new version writes it, in place of the old one's in the copy.
                            const auto written = dir.parent_path() / ("." + dir.filename().string() + ".moved");
                            fs::remove_all(written);
                            opened->saveSnapshot(written.string());
                            fs::remove_all(dir);
                            fs::rename(written, dir);
                        }
                    }
                    with[detail::member(node, "plugin")] = "{\"format\":" + detail::quote(t.plugin.format) + ",\"id\":" + detail::quote(t.plugin.id) + ",\"version\":" + detail::quote(t.to) + ",\"pathHint\":" + detail::quote(t.build.string()) + '}';
                }
                r.status = !t.claimed ? "noClaim" : r.changes.empty() ? "movable" : "changed";
                if(!t.move) return out;
                // The lock: this plugin's entries for the old version and the new go, the new version's builds here come in.
                const V lock = detail::member(doc.get(), "lock");
                std::string entries;
                for(const auto e : elements(detail::member(lock, "plugins")))
                {
                    const auto version = text(detail::member(e, "version"));
                    if(text(detail::member(e, "format")) == t.plugin.format && text(detail::member(e, "id")) == t.plugin.id && (version == t.plugin.version || version == t.to)) continue;
                    if(!entries.empty()) entries += ',';
                    write(entries, e, {});
                }
                for(const auto& e : t.lock) entries += (entries.empty() ? "" : ",") + e;
                if(lock) with[lock] = "{\"plugins\":[" + entries + "]}";
                std::string rewritten;
                write(rewritten, doc.get(), with);
                const auto file = copy / type->document;
                writeAtomically(file, rewritten + '\n');
                // A new revision, its parent the one moved, and the content hash of what it holds now.
                auto info = readInfo(file, type->format);
                info.parent = info.revision; info.revision = fresh();
                info.hash = contentHash(file);
                writeInfo(file, info);
                out.wrote = info.revision; out.copy = copy;
                r.status = "moved"; // once the control thread swaps it in
            }
            catch(const std::exception& e)
            {
                r.status = "refused"; r.reason = e.what(); r.changes.clear();
                std::error_code ignored; if(t.move) fs::remove_all(copy, ignored);
                out.copy.clear();
            }
            return out;
        }
    }

    struct Session::VersionWork
    {
        std::mutex mutex;
        uint64_t id = 0;
        VersionJob job;                 // guarded by mutex
        std::vector<Checked> checked;   // guarded by mutex: the move's copies, for the control thread to swap in
        bool finished = false;          // guarded by mutex: the thread is done
        bool swapped = false;           // control thread: the copies are in place (or given up)
        Target target;
        fs::path stage;                 // the move's copies, in the data folder
        std::atomic<bool> cancel = false;
        std::thread thread;
        ~VersionWork()
        {
            cancel = true;
            if(thread.joinable()) thread.join();
            std::error_code e; if(!stage.empty()) fs::remove_all(stage, e);
        }
    };

    uint64_t Session::planVersionMove(const PluginVersion& plugin, const std::string& to, const VersionScope& scope)
    {
        auto& l = library();
        std::vector<LibraryItem> items;
        std::vector<std::string> unknown; // asked for by ID and not in the library
        std::set<std::string> ids(scope.items.begin(), scope.items.end());
        if(!scope.setlist.empty()) for(const auto& id : setlistPerformances(setlist(scope.setlist))) ids.insert(id);
        if(ids.empty() && scope.library.empty() && !scope.all) throw std::invalid_argument("which items: items, library, setlist or all");
        for(const auto& id : ids) if(!l.find(id)) unknown.push_back(id);
        for(const auto& i : l.items())
            if(scope.all || ids.count(i.id) || (!scope.library.empty() && (i.origin.library == scope.library || i.library == scope.library))) items.push_back(i);
        return startVersionJob(plugin, to, std::move(items), std::move(unknown), false, false);
    }

    uint64_t Session::moveVersions(const PluginVersion& plugin, const std::string& to, const std::vector<std::string>& items, bool withoutClaim)
    {
        // Not during a show (RFC 0027): at instrument and pass-through, not while the transport runs.
        if(const char* level = std::getenv("OSCAIRN_RELIABILITY"); level && (std::string(level) == "instrument" || std::string(level) == "pass-through") && playing_.load())
            throw std::runtime_error("Stop the transport to move sounds");
        if(items.empty()) throw std::invalid_argument("which items to move");
        auto& l = library();
        std::vector<LibraryItem> found;
        std::vector<std::string> unknown;
        for(const auto& id : std::set<std::string>(items.begin(), items.end())) if(const auto* i = l.find(id)) found.push_back(*i); else unknown.push_back(id);
        return startVersionJob(plugin, to, std::move(found), std::move(unknown), true, withoutClaim);
    }

    uint64_t Session::startVersionJob(const PluginVersion& plugin, const std::string& to, std::vector<LibraryItem> items, std::vector<std::string> unknown, bool move, bool withoutClaim)
    {
        if(versionWork_ && versionJob(versionWork_->id).state == "running") throw std::logic_error("a version job is running; wait for it to finish");
        if((plugin.format != "clap" && plugin.format != "lv2") || plugin.id.empty() || plugin.version.empty() || to.empty()) throw std::invalid_argument("name the plugin by format, ID and version, and the version to move to");
        if(!assets::older(plugin.version, to)) throw std::invalid_argument("only a newer version than " + plugin.version + " can be moved to; going back is undoing the move");
        if(items.size() + unknown.size() > mostItems) throw std::invalid_argument("at most " + std::to_string(mostItems) + " items at once");
        Target t;
        t.plugin = plugin; t.to = to; t.move = move; t.rate = sampleRate_; t.frames = maxFrames_; t.assets = assets_.root();
        const auto builds = storedBuilds(plugin.format, plugin.id, to);
        if(!builds.count(platform())) throw std::invalid_argument(plugin.id + " " + to + " is not installed here");
        t.build = builds.at(platform());
        for(const auto& [forPlatform, build] : builds)
            t.lock.push_back("{\"format\":" + detail::quote(plugin.format) + ",\"id\":" + detail::quote(plugin.id) + ",\"version\":" + detail::quote(to) + ",\"platform\":" + detail::quote(forPlatform) + ",\"sha256\":" + detail::quote(bundleDigest(build)) + '}');
        const auto old = storedBuilds(plugin.format, plugin.id, plugin.version);
        if(old.count(platform())) t.old = old.at(platform());
        const auto claim = claimFor(plugin, to);
        t.claimed = claim.has_value() || (move && withoutClaim);
        // Without the publisher's claim, one item at a time, and only when asked so (RFC 0043 decision 3).
        if(move && !claim && (items.size() + unknown.size() != 1 || !withoutClaim))
            throw std::invalid_argument(plugin.id + " " + to + " does not say it opens sounds saved with " + plugin.version + ": move one item at a time, without the claim");
        auto work = std::make_shared<VersionWork>();
        work->id = work->job.id = ++versionJobs_;
        work->job.state = "running"; work->job.total = uint32_t(items.size()); work->job.claim = claim;
        for(const auto& id : unknown) work->job.items.push_back({id, {}, {}, "refused", "not in the library"});
        work->target = t;
        if(move) { removeStaleStaging(dataHome()); work->stage = stagingFolder("move"); fs::create_directories(work->stage); }
        auto* w = work.get();
        versionWork_.reset(); // the last job's thread is done; joined here
        w->thread = std::thread([w, items = std::move(items)] {
            assets::Store assets(w->target.assets);
            for(size_t n = 0; n < items.size() && !w->cancel; ++n)
            {
                std::optional<Checked> c;
                try { c = check(items[n], w->target, w->stage / std::to_string(n), assets); }
                catch(const std::exception& e) { c = Checked{{items[n].id, items[n].name, items[n].kind, "refused", e.what()}}; }
                std::lock_guard lock(w->mutex);
                ++w->job.checked;
                if(c) { w->job.items.push_back(c->item); if(!c->copy.empty()) w->checked.push_back(std::move(*c)); }
            }
            std::lock_guard lock(w->mutex);
            w->finished = true;
        });
        versionWork_ = std::move(work);
        return versionJobs_;
    }

    Control::VersionJob Session::versionJob(uint64_t id)
    {
        if(!versionWork_ || versionWork_->id != id) throw std::invalid_argument("no version job " + std::to_string(id));
        auto& w = *versionWork_;
        std::unique_lock lock(w.mutex);
        if(!w.finished || w.swapped) return w.job;
        lock.unlock();
        if(w.thread.joinable()) w.thread.join();
        w.swapped = true;
        // Swapped in here, on the control thread, which owns the library: each item whose revision is
        // still the one the job read goes whole under Moves/<move>/ and its copy takes its place.
        auto& l = library();
        std::vector<std::string> kept;
        std::string move;
        for(auto& c : w.checked)
        {
            auto* r = &*std::find_if(w.job.items.begin(), w.job.items.end(), [&](const auto& i) { return i.id == c.item.id; });
            const auto* now = l.find(c.item.id);
            if(!now || now->revision != c.was) { r->status = "savedSince"; r->changes.clear(); continue; }
            if(move.empty())
            {
                move = date().substr(0, 10) + "-" + fresh().substr(0, 8);
                fs::create_directories(movesFolder() / move);
            }
            const fs::path at = now->path, aside = movesFolder() / move / c.item.id;
            try
            {
                fs::rename(at, aside);
                try { fs::rename(c.copy, at); }
                catch(...) { fs::rename(aside, at); throw; }
                kept.push_back("{\"id\":" + detail::quote(c.item.id) + ",\"kind\":" + detail::quote(c.item.kind) + ",\"revision\":" + detail::quote(c.wrote) + ",\"path\":" + detail::quote(at.string()) + '}');
            }
            catch(const std::exception& e) { r->status = "refused"; r->reason = std::string("it could not be put in place: ") + e.what(); r->changes.clear(); }
        }
        if(!move.empty())
        {
            if(kept.empty()) { std::error_code e; fs::remove_all(movesFolder() / move, e); move.clear(); }
            else
            {
                std::string items;
                for(const auto& k : kept) items += (items.empty() ? "" : ",") + k;
                const auto& p = w.target.plugin;
                writeAtomically(movesFolder() / move / "move.json", "{\"plugin\":{\"format\":" + detail::quote(p.format) + ",\"id\":" + detail::quote(p.id) + ",\"version\":" + detail::quote(p.version) + "},\"to\":" + detail::quote(w.target.to) +
                                                                     ",\"date\":" + detail::quote(date()) + ",\"items\":[" + items + "]}\n");
                l.rescan();
            }
        }
        std::error_code e; fs::remove_all(w.stage, e);
        lock.lock();
        w.job.move = move;
        w.job.state = "done";
        return w.job;
    }

    namespace
    {
        struct KeptMove { std::string move, date; Control::PluginVersion plugin; std::string to; struct Item { std::string id, kind, revision, path; }; std::vector<Item> items; };
        KeptMove readMove(const std::string& move)
        {
            if(move.empty() || move.find('/') != std::string::npos || move.front() == '.') throw std::invalid_argument("no move " + move);
            KeptMove m; m.move = move;
            std::string bytes;
            try { bytes = slurp(movesFolder() / move / "move.json"); } catch(const std::exception&) { throw std::invalid_argument("no move " + move); }
            const auto doc = parse(bytes);
            const V p = detail::member(doc.get(), "plugin");
            m.plugin = {text(detail::member(p, "format")), text(detail::member(p, "id")), text(detail::member(p, "version"))};
            m.to = text(detail::member(doc.get(), "to")); m.date = text(detail::member(doc.get(), "date"));
            for(const auto i : elements(detail::member(doc.get(), "items")))
                m.items.push_back({text(detail::member(i, "id")), text(detail::member(i, "kind")), text(detail::member(i, "revision")), text(detail::member(i, "path"))});
            return m;
        }
    }

    std::vector<Control::VersionMove> Session::versionMoves()
    {
        auto& l = library();
        std::vector<VersionMove> out;
        std::error_code e;
        for(const auto& entry : fs::directory_iterator(movesFolder(), e))
            try
            {
                const auto m = readMove(entry.path().filename().string());
                VersionMove v{m.move, m.date, m.plugin, m.to, {}, false};
                for(const auto& i : m.items)
                {
                    v.items.push_back(i.id);
                    if(const auto* now = l.find(i.id); now && now->revision == i.revision) v.undoable = true;
                }
                out.push_back(std::move(v));
            }
            catch(const std::exception&) {} // not a move's folder
        std::sort(out.begin(), out.end(), [](const auto& a, const auto& b) { return a.date > b.date; });
        return out;
    }

    std::vector<Control::VersionUndone> Session::undoVersionMove(const std::string& move)
    {
        if(versionWork_ && versionJob(versionWork_->id).state == "running") throw std::logic_error("a version job is running; wait for it to finish");
        auto m = readMove(move);
        auto& l = library();
        std::vector<VersionUndone> out;
        std::vector<KeptMove::Item> left;
        for(const auto& i : m.items)
        {
            const auto* now = l.find(i.id);
            const fs::path aside = movesFolder() / move / i.id;
            if(!now || now->revision != i.revision || !fs::is_directory(aside)) { out.push_back({i.id, true}); left.push_back(i); continue; }
            // The moved item goes, the one kept takes its place: the folder exactly as it was.
            const fs::path at = now->path, gone = movesFolder() / move / ("." + i.id + ".undone");
            std::error_code e; fs::remove_all(gone, e);
            fs::rename(at, gone);
            try { fs::rename(aside, at); } catch(...) { fs::rename(gone, at); throw; }
            fs::remove_all(gone, e);
            out.push_back({i.id, false});
        }
        // What is left of the move, kept for forgetting; none: the move is gone.
        std::error_code e;
        if(std::none_of(out.begin(), out.end(), [](const auto& u) { return u.savedSince; })) fs::remove_all(movesFolder() / move, e);
        else
        {
            std::string items;
            for(const auto& i : left) items += std::string(items.empty() ? "" : ",") + "{\"id\":" + detail::quote(i.id) + ",\"kind\":" + detail::quote(i.kind) + ",\"revision\":" + detail::quote(i.revision) + ",\"path\":" + detail::quote(i.path) + '}';
            writeAtomically(movesFolder() / move / "move.json", "{\"plugin\":{\"format\":" + detail::quote(m.plugin.format) + ",\"id\":" + detail::quote(m.plugin.id) + ",\"version\":" + detail::quote(m.plugin.version) + "},\"to\":" + detail::quote(m.to) +
                                                                 ",\"date\":" + detail::quote(m.date) + ",\"items\":[" + items + "]}\n");
        }
        l.rescan();
        return out;
    }

    void Session::forgetVersionMove(const std::string& move)
    {
        readMove(move); // refuses one that is not there
        fs::remove_all(movesFolder() / move);
    }
}
