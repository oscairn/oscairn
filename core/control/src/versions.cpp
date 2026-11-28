// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Moving library items to a newer installed version of a plugin (RFC 0043 step 2). A job off the
// control thread opens each node that uses the plugin at the old version with the new version's
// build (always in a worker of its own, offline, with the worker's memory limit and reply timeout,
// whatever the placement policy says), restores the node's saved state, compares its ports with the
// document's and its parameters with the old build's, and for a move saves the state again as the
// new version writes it (decided by the owner 2026-11-28: the kept-warm take matches a state
// against its file, so the file must hold the new version's state). The job works on a copy of
// each item; the control thread swaps the copy in whole and keeps the item as it was under
// Moves/<move>/ in the data folder, so nothing is lost and undo is putting the folder back; every
// rename runs under a journal written first, settled at the next start after a crash.
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
#include <atomic>
#include <chrono>
#include <csignal>
#include <fcntl.h>
#include <fstream>
#include <mutex>
#include <set>
#include <sys/stat.h>
#include <unistd.h>
#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif
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
        // version it covers, both dotted numbers, by a key the trust store trusts for plugins and has not
        // revoked, now, that signed `to`'s build here and an installed build of `from`: the same publisher.
        std::optional<Control::VersionClaim> claimFor(const Control::PluginVersion& p, const std::string& to, const std::set<std::string>& trusted)
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
            if(c.opensStateFrom.empty() || c.key.empty() || !trusted.count(c.key) || !dottedVersion(p.version) || !dottedVersion(to) || assets::older(p.version, c.opensStateFrom)) return std::nullopt;
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
        // The plugin worker program a job opens plugins in, apart from the engine, whatever the placement policy: the
        // policy's, else one beside this program, else the one built with it; empty when none is found.
        std::string isolatedWorker()
        {
            if(!placementPolicy().worker.empty()) return placementPolicy().worker;
            fs::path self;
#ifdef __APPLE__
            char path[4096]; uint32_t size = sizeof path;
            if(_NSGetExecutablePath(path, &size) == 0) self = path;
#else
            std::error_code e; self = fs::read_symlink("/proc/self/exe", e);
#endif
            std::error_code e;
            if(!self.empty() && fs::is_regular_file(self.parent_path() / "oscairn-plugin-worker", e)) return (self.parent_path() / "oscairn-plugin-worker").string();
#ifdef OSCAIRN_VERSIONS_WORKER
            if(fs::is_regular_file(OSCAIRN_VERSIONS_WORKER, e)) return OSCAIRN_VERSIONS_WORKER;
#endif
            return {};
        }
        // The job's worker processes, so a cancel can end the one an item waits on.
        struct Watch
        {
            std::mutex mutex;
            std::set<long> pids;
            std::atomic<bool> cancel = false;
            void stop() { cancel = true; std::lock_guard lock(mutex); for(const auto pid : pids) if(pid > 0) ::kill(pid, SIGKILL); }
        };

        // What the job needs, read on the control thread before it starts.
        struct Target
        {
            Control::PluginVersion plugin; std::string to;
            fs::path build, old;                // the build of `to` here, and of `from` when it is here (else empty)
            std::vector<std::string> lock;      // `to`'s lock entries: each platform's build in the store, by digest
            bool claimed = false, move = false;
            double rate = 48000; uint32_t frames = 512;
            fs::path assets;                    // the asset store's root, for plugins whose state names samples
            std::string worker;                 // the plugin worker program (isolatedWorker)
            Watch* watch = nullptr;             // the job's, while it runs; none on the control thread
        };
        // One item checked, and for a move its rewritten copy: the revision it was read at and the one written.
        // path: the item's folder; inside: the same, relative to the data folder, as a kept move records it.
        struct Checked { Control::VersionItem item; fs::path copy; std::string was, wrote; fs::path path = {}; std::string inside = {}; };
        // A folder relative to the data folder, empty when it is not inside it.
        std::string insideData(const fs::path& folder)
        {
            std::error_code e;
            const auto relative = fs::weakly_canonical(folder, e).lexically_relative(fs::weakly_canonical(dataHome(), e));
            if(e || relative.empty() || *relative.begin() == ".." || relative.is_absolute()) return {};
            return relative.generic_string();
        }

        // A build opened in a worker of its own, offline; while the job holds it, a cancel can kill it.
        struct Opened
        {
            std::shared_ptr<PluginHost> host;
            Watch* watch = nullptr; long pid = 0;
            Opened(std::shared_ptr<PluginHost> h, Watch* w) : host(std::move(h)), watch(w) {}
            Opened(Opened&& o) noexcept : host(std::move(o.host)), watch(std::exchange(o.watch, nullptr)), pid(o.pid) {}
            Opened& operator=(Opened&&) = delete;
            PluginHost* operator->() const { return host.get(); }
            ~Opened() { host.reset(); if(watch) { std::lock_guard lock(watch->mutex); watch->pids.erase(pid); } }
        };
        Opened open(const fs::path& build, const std::string& id, const std::string& version, const Target& t, assets::Store& assets)
        {
            if(t.worker.empty()) throw std::runtime_error("no plugin worker program was found to open plugins in, apart from the engine");
            if(t.watch && t.watch->cancel) throw std::runtime_error("canceled");
            Opened out{loadPluginInWorker(t.worker, build.string(), id, t.rate, t.frames, ProcessingMode::Offline, placementPolicy().memoryLimit), t.watch};
            out.pid = out.host->workerPid();
            if(t.watch) { std::lock_guard lock(t.watch->mutex); t.watch->pids.insert(out.pid); if(t.watch->cancel) ::kill(out.pid, SIGKILL); }
            auto& host = out.host;
            if(host->descriptor().version != version) throw std::runtime_error("the build here is version " + host->descriptor().version + ", not " + version);
            host->setAssets(&assets);
            // As the graph holds it, so its snapshot is the one the graph saved (an instrument's pool of prepared sounds).
            host = preparedInstrument(std::move(host), t.rate, t.frames, ProcessingMode::Offline, {});
            host->setAssets(&assets);
            return out;
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
            out.path = folder;
            try
            {
                // The item as it was is kept under Moves/ by a rename, so it must be on the data folder's drive, and inside the
                // data folder, so a kept move names it relative to it (a data folder copied elsewhere still finds its items).
                struct stat a{}, b{};
                if(::stat(folder.parent_path().c_str(), &a) != 0 || ::stat(dataHome().c_str(), &b) != 0 || a.st_dev != b.st_dev)
                    throw std::runtime_error("its folder is on another drive than the data folder, where a move keeps the item as it was");
                out.inside = insideData(folder);
                if(out.inside.empty()) throw std::runtime_error("its folder is outside the data folder, where a move keeps the item as it was");
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
                    if(t.watch && t.watch->cancel) throw std::runtime_error("canceled");
                    if(!dir.empty())
                    {
                        auto opened = open(t.build, t.plugin.id, t.to, t, assets);
                        if(!shared && (!samePorts(detail::member(node, "inputs"), opened->inputPorts()) || !samePorts(detail::member(node, "outputs"), opened->outputPorts()) ||
                                       whole(detail::member(node, "noteInputs")) != opened->noteInputPorts() || whole(detail::member(node, "noteOutputs")) != opened->noteOutputPorts()))
                            throw std::runtime_error(name + ": its audio or note ports are not the same in " + t.to);
                        try { opened->loadSnapshot(dir.string()); }
                        catch(const std::exception& e) { throw std::runtime_error(name + ": its saved state does not open in " + t.to + " (" + e.what() + ")"); }
                        const auto after = values(*opened.host);
                        // Compared with what the old version reads from the same state, when its build is here.
                        if(!t.old.empty())
                        {
                            std::map<clap_id, std::pair<std::string, double>> before;
                            {
                                auto older = open(t.old, t.plugin.id, t.plugin.version, t, assets);
                                try { older->loadSnapshot(dir.string()); }
                                catch(const std::exception& e) { throw std::runtime_error(name + ": " + t.plugin.version + " itself does not open its saved state (" + e.what() + ")"); }
                                before = values(*older.host);
                            }
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
        Watch watch;                    // its worker processes, for a cancel
        std::thread thread;
        ~VersionWork()
        {
            watch.stop(); // an item waiting on a worker fails at once
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
        t.worker = isolatedWorker();
        if(t.worker.empty()) throw std::runtime_error("no plugin worker program was found to open plugins in, apart from the engine");
        const auto builds = storedBuilds(plugin.format, plugin.id, to);
        if(!builds.count(platform())) throw std::invalid_argument(plugin.id + " " + to + " is not installed here");
        t.build = builds.at(platform());
        for(const auto& [forPlatform, build] : builds)
            t.lock.push_back("{\"format\":" + detail::quote(plugin.format) + ",\"id\":" + detail::quote(plugin.id) + ",\"version\":" + detail::quote(to) + ",\"platform\":" + detail::quote(forPlatform) + ",\"sha256\":" + detail::quote(bundleDigest(build)) + '}');
        const auto old = storedBuilds(plugin.format, plugin.id, plugin.version);
        if(old.count(platform())) t.old = old.at(platform());
        std::set<std::string> trusted; // the keys trusted for plugins and not revoked, now
        for(const auto& k : trustedKeys()) if(!k.revoked && std::count(k.scope.begin(), k.scope.end(), "plugins")) trusted.insert(k.key);
        const auto claim = claimFor(plugin, to, trusted);
        t.claimed = claim.has_value() || (move && withoutClaim);
        // Without the publisher's claim, one item at a time, and only when asked so (RFC 0043 decision 3).
        if(move && !claim && (items.size() + unknown.size() != 1 || !withoutClaim))
            throw std::invalid_argument(plugin.id + " " + to + " does not say it opens sounds saved with " + plugin.version + ": move one item at a time, without the claim");
        auto work = std::make_shared<VersionWork>();
        work->id = work->job.id = ++versionJobs_;
        work->job.state = "running"; work->job.total = uint32_t(items.size()); work->job.claim = claim;
        for(const auto& id : unknown) work->job.items.push_back({id, {}, {}, "refused", "not in the library"});
        // The open Performance's own item: saving it would write the old versions over the moved item.
        std::erase_if(items, [&](const LibraryItem& i) {
            if(info_.id.empty() || i.id != info_.id || !i.library.empty()) return false;
            work->job.items.push_back({i.id, i.name, i.kind, "refused", "it is the open Performance: move its parts with parts.useVersion and save it, or open another first"});
            return true;
        });
        work->job.total = uint32_t(items.size());
        work->target = t;
        work->target.watch = &work->watch;
        if(move) { removeStaleStaging(dataHome()); work->stage = stagingFolder("move"); fs::create_directories(work->stage); }
        auto* w = work.get();
        versionWork_.reset(); // the last job's thread is done; joined here
        w->thread = std::thread([w, items = std::move(items)] {
            try
            {
                assets::Store assets(w->target.assets);
                for(size_t n = 0; n < items.size() && !w->watch.cancel; ++n)
                {
                    std::optional<Checked> c;
                    try { c = check(items[n], w->target, w->stage / std::to_string(n), assets); }
                    catch(const std::exception& e) { c = Checked{{items[n].id, items[n].name, items[n].kind, "refused", e.what()}}; }
                    std::lock_guard lock(w->mutex);
                    ++w->job.checked;
                    if(c) { w->job.items.push_back(c->item); if(!c->copy.empty()) w->checked.push_back(std::move(*c)); }
                }
            }
            catch(const std::exception& e) { std::lock_guard lock(w->mutex); w->job.error = e.what(); }
            std::lock_guard lock(w->mutex);
            w->finished = true;
        });
        versionWork_ = std::move(work);
        return versionJobs_;
    }

    namespace
    {
        // Moves swap folders and undo puts them back under a journal, Moves/<move>/move.json, written
        // durably before the first rename and settled from what is on disk: so a crash or a failure at
        // any step leaves every item in the library, old or new, and a later start finishes or rolls
        // back each one (the owner's rule: nothing a user made is lost or unreachable). One at a time
        // in the process: Sessions share the data folder.
        std::mutex journalMutex;
        void syncPath(const fs::path& p) { const int fd = ::open(p.c_str(), O_RDONLY); if(fd >= 0) { ::fsync(fd); ::close(fd); } }
        void writeDurably(const fs::path& file, const std::string& text)
        {
            const auto temporary = file.parent_path() / ("." + file.filename().string() + ".tmp");
            {
                std::ofstream out(temporary, std::ios::binary | std::ios::trunc);
                out << text;
                out.close();
                if(!out) throw std::runtime_error("cannot write " + file.string());
            }
            syncPath(temporary);
            fs::rename(temporary, file);
            syncPath(file.parent_path());
        }
        // OSCAIRN_MOVE_CRASH_AT=N ends the process at the Nth journal step (fault injection for tests).
        void crashPoint()
        {
            static const long at = [] { const char* v = std::getenv("OSCAIRN_MOVE_CRASH_AT"); return v ? std::atol(v) : 0L; }();
            static long count = 0;
            if(at > 0 && ++count == at) std::_Exit(86);
        }
        void renameDurably(const fs::path& from, const fs::path& to)
        {
            fs::rename(from, to);
            syncPath(to.parent_path());
            if(from.parent_path() != to.parent_path()) syncPath(from.parent_path());
            crashPoint();
        }
        struct Journal
        {
            std::string move, date, state; Control::PluginVersion plugin; std::string to;
            struct Item { std::string id, kind, revision, path; };
            std::vector<Item> items;
            std::vector<std::string> undoing;
        };
        fs::path journalOf(const std::string& move) { return movesFolder() / move / "move.json"; }
        // Where an item a journal names is: its path is relative to the data folder.
        fs::path where(const std::string& relative) { return dataHome() / relative; }
        Journal readJournal(const std::string& move)
        {
            if(move.empty() || move.find('/') != std::string::npos || move.front() == '.') throw std::invalid_argument("no move " + move);
            Journal j; j.move = move;
            std::string bytes;
            try { bytes = slurp(journalOf(move)); } catch(const std::exception&) { throw std::invalid_argument("no move " + move); }
            const auto doc = parse(bytes);
            const V p = detail::member(doc.get(), "plugin");
            j.plugin = {text(detail::member(p, "format")), text(detail::member(p, "id")), text(detail::member(p, "version"))};
            j.to = text(detail::member(doc.get(), "to")); j.date = text(detail::member(doc.get(), "date"));
            j.state = text(detail::member(doc.get(), "state"));
            if(j.state.empty()) j.state = "done";
            for(const auto i : elements(detail::member(doc.get(), "items")))
                j.items.push_back({text(detail::member(i, "id")), text(detail::member(i, "kind")), text(detail::member(i, "revision")), text(detail::member(i, "path"))});
            for(const auto i : elements(detail::member(doc.get(), "undoing"))) j.undoing.push_back(text(i));
            for(const auto& i : j.items)
            {
                const fs::path path = i.path;
                if(i.id.empty() || i.id.find('/') != std::string::npos || i.id.front() == '.' || path.empty() || path.is_absolute() ||
                   std::any_of(path.begin(), path.end(), [](const auto& c) { return c == ".." || c == "."; }))
                    throw std::runtime_error("move " + move + " names an item it cannot keep");
            }
            return j;
        }
        void writeJournal(const Journal& j)
        {
            std::string items, undoing;
            for(const auto& i : j.items) items += std::string(items.empty() ? "" : ",") + "{\"id\":" + detail::quote(i.id) + ",\"kind\":" + detail::quote(i.kind) + ",\"revision\":" + detail::quote(i.revision) + ",\"path\":" + detail::quote(i.path) + '}';
            for(const auto& u : j.undoing) undoing += (undoing.empty() ? "" : ",") + detail::quote(u);
            writeDurably(journalOf(j.move), "{\"plugin\":{\"format\":" + detail::quote(j.plugin.format) + ",\"id\":" + detail::quote(j.plugin.id) + ",\"version\":" + detail::quote(j.plugin.version) + "},\"to\":" + detail::quote(j.to) +
                                                ",\"date\":" + detail::quote(j.date) + ",\"state\":" + detail::quote(j.state) + ",\"items\":[" + items + "],\"undoing\":[" + undoing + "]}\n");
            crashPoint();
        }
        // A move being swapped, settled from what is on disk: an item whose old folder went aside and whose copy is not in
        // place yet takes its copy (forward), one whose copy is gone takes its old folder back; one not started stays. The
        // items swapped are recorded and the move is done; none: the move goes. Each step is safe to run again.
        std::set<std::string> settleSwap(Journal& j)
        {
            const auto dir = movesFolder() / j.move;
            std::set<std::string> swapped;
            std::vector<Journal::Item> kept;
            for(const auto& i : j.items)
            {
                std::error_code e;
                const fs::path at = where(i.path), aside = dir / i.id, copy = dir / (i.id + ".new");
                const bool here = fs::exists(at, e), set = fs::exists(aside, e), ready = fs::exists(copy, e);
                if(here && set) { if(ready) fs::remove_all(copy, e); }
                else if(here) { if(ready) fs::remove_all(copy, e); continue; }              // not started: the item as it was
                else if(ready) renameDurably(copy, at);                                    // forward: the copy takes its place
                else if(set) { renameDurably(aside, at); continue; }                       // no copy: the old folder back
                else continue;                                                            // neither: nothing here to keep
                swapped.insert(i.id);
                kept.push_back(i);
            }
            j.items = std::move(kept);
            j.state = "done";
            std::error_code e;
            if(j.items.empty()) fs::remove_all(dir, e);
            else writeJournal(j);
            return swapped;
        }
        // An undo being put back, settled the same way: an item whose moved folder went aside takes its kept folder (forward)
        // or, that gone, its moved folder back. Items put back leave the move; none left: the move goes.
        std::set<std::string> settleUndo(Journal& j)
        {
            const auto dir = movesFolder() / j.move;
            std::set<std::string> undone;
            for(const auto& id : j.undoing)
            {
                const auto i = std::find_if(j.items.begin(), j.items.end(), [&](const auto& x) { return x.id == id; });
                if(i == j.items.end()) continue;
                std::error_code e;
                const fs::path at = where(i->path), kept = dir / id, moved = dir / ("." + id + ".undone");
                const bool here = fs::exists(at, e), set = fs::exists(kept, e), gone = fs::exists(moved, e);
                if(here && !set) { if(gone) fs::remove_all(moved, e); undone.insert(id); continue; } // put back
                if(here) continue;                                                                 // not started
                if(set) { renameDurably(kept, at); if(gone) fs::remove_all(moved, e); undone.insert(id); continue; }
                if(gone) renameDurably(moved, at);                                                 // the moved folder back
            }
            std::erase_if(j.items, [&](const auto& i) { return undone.count(i.id); });
            j.undoing.clear();
            std::error_code e;
            if(j.items.empty()) fs::remove_all(dir, e);
            else writeJournal(j);
            return undone;
        }
    }

    void Session::recoverVersionMoves()
    {
        std::lock_guard guard(journalMutex);
        std::error_code e;
        bool changed = false;
        for(const auto& entry : fs::directory_iterator(movesFolder(), e))
            try
            {
                auto j = readJournal(entry.path().filename().string());
                if(j.state == "swapping") { settleSwap(j); changed = true; }
                else if(!j.undoing.empty()) { settleUndo(j); changed = true; }
            }
            catch(const std::exception& x) { std::fprintf(stderr, "oscairn: the kept move %s could not be settled: %s\n", entry.path().filename().string().c_str(), x.what()); }
        if(changed) library().rescan();
    }

    Control::VersionJob Session::versionJob(uint64_t id)
    {
        if(!versionWork_ || versionWork_->id != id) throw std::invalid_argument("no version job " + std::to_string(id));
        auto& w = *versionWork_;
        std::unique_lock lock(w.mutex);
        if(!w.finished || w.swapped) return w.job;
        lock.unlock();
        if(w.thread.joinable()) w.thread.join();
        w.swapped = true; // from here the job ends, whatever happens: done, or failed with what was done recorded
        auto& l = library();
        auto status = [&](const std::string& item) -> VersionItem& { return *std::find_if(w.job.items.begin(), w.job.items.end(), [&](const auto& i) { return i.id == item; }); };
        std::string error = w.job.error; // the job's own failure, if it had one
        Journal j;
        try
        {
            // Each item whose revision is still the one the job read; one saved since is left.
            std::vector<Checked*> ready;
            for(auto& c : w.checked)
            {
                const auto* now = l.find(c.item.id);
                if(!now || now->revision != c.was || fs::path(now->path) != fs::path(c.path)) { status(c.item.id).status = "savedSince"; status(c.item.id).changes.clear(); }
                else ready.push_back(&c);
            }
            if(!ready.empty())
            {
                std::lock_guard guard(journalMutex);
                j.move = date().substr(0, 10) + "-" + fresh().substr(0, 8); j.date = date(); j.state = "swapping";
                j.plugin = w.target.plugin; j.to = w.target.to;
                const auto dir = movesFolder() / j.move;
                fs::create_directories(dir);
                // The copies beside the journal first, so a settle can always find them.
                for(const auto* c : ready) { fs::rename(c->copy, dir / (c->item.id + ".new")); j.items.push_back({c->item.id, c->item.kind, c->wrote, c->inside}); }
                writeJournal(j);
                for(const auto* c : ready)
                {
                    renameDurably(c->path, dir / c->item.id);
                    renameDurably(dir / (c->item.id + ".new"), c->path);
                }
                j.state = "done";
                writeJournal(j);
            }
        }
        catch(const std::exception& e)
        {
            error = e.what();
            // What was done is settled from the disk and recorded; a journal that cannot be written is settled at the next start.
            try { std::lock_guard guard(journalMutex); if(!j.move.empty() && fs::exists(journalOf(j.move))) { auto now = readJournal(j.move); settleSwap(now); j = now; } else { std::error_code x; if(!j.move.empty()) fs::remove_all(movesFolder() / j.move, x); j = {}; } }
            catch(const std::exception& x) { error += std::string("; and settling it failed (the next start settles it): ") + x.what(); }
        }
        std::set<std::string> moved;
        for(const auto& i : j.items) moved.insert(i.id);
        for(auto& c : w.checked)
            if(auto& r = status(c.item.id); r.status == "moved" && !moved.count(c.item.id)) { r.status = "refused"; r.reason = error.empty() ? "it was not put in place" : "it could not be put in place: " + error; r.changes.clear(); }
        try { if(!j.items.empty()) l.rescan(); } catch(const std::exception& e) { if(error.empty()) error = e.what(); }
        std::error_code e; fs::remove_all(w.stage, e);
        lock.lock();
        w.job.move = j.items.empty() ? std::string() : j.move;
        w.job.state = error.empty() ? "done" : "failed";
        w.job.error = error;
        return w.job;
    }

    std::vector<Control::VersionMove> Session::versionMoves()
    {
        auto& l = library();
        std::vector<VersionMove> out;
        std::error_code e;
        for(const auto& entry : fs::directory_iterator(movesFolder(), e))
            try
            {
                const auto m = readJournal(entry.path().filename().string());
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
        auto& l = library();
        std::vector<VersionUndone> out;
        {
            std::lock_guard guard(journalMutex);
            auto j = readJournal(move);
            if(j.state != "done") throw std::runtime_error("the move " + move + " is not settled");
            // Which items: each the move wrote and nothing saved since, recorded before the first rename.
            for(const auto& i : j.items)
            {
                const auto* now = l.find(i.id);
                std::error_code e;
                if(now && now->revision == i.revision && insideData(now->path) == i.path && fs::is_directory(movesFolder() / move / i.id, e)) j.undoing.push_back(i.id);
            }
            writeJournal(j);
            const auto items = j.items;
            try
            {
                for(const auto& id : j.undoing)
                {
                    const auto& i = *std::find_if(j.items.begin(), j.items.end(), [&](const auto& x) { return x.id == id; });
                    renameDurably(where(i.path), movesFolder() / move / ("." + id + ".undone"));
                    renameDurably(movesFolder() / move / id, where(i.path));
                }
            }
            catch(...) { try { settleUndo(j); } catch(const std::exception&) {} l.rescan(); throw; } // what was put back stays, the rest as it was; else the next start settles it
            const auto undone = settleUndo(j);
            for(const auto& i : items) out.push_back({i.id, !undone.count(i.id)});
        }
        l.rescan();
        return out;
    }

    void Session::forgetVersionMove(const std::string& move)
    {
        std::lock_guard guard(journalMutex);
        if(readJournal(move).state != "done") throw std::runtime_error("the move " + move + " is not settled");
        fs::remove_all(movesFolder() / move);
    }

    Control::VersionUsed Session::useVersion(const PluginVersion& plugin, const std::string& to, const std::vector<NodeId>& asked, bool withoutClaim)
    {
        if(staged_) throw std::invalid_argument("Finish pending graph edits before moving plugins to another version");
        if(!assets::older(plugin.version, to)) throw std::invalid_argument("only a newer version than " + plugin.version + " can be used; going back is undoing the edit");
        const auto builds = storedBuilds(plugin.format, plugin.id, to);
        if(!builds.count(platform())) throw std::invalid_argument(plugin.id + " " + to + " is not installed here");
        const auto build = builds.at(platform());
        std::set<std::string> trusted; // the keys trusted for plugins and not revoked, now
        for(const auto& k : trustedKeys()) if(!k.revoked && std::count(k.scope.begin(), k.scope.end(), "plugins")) trusted.insert(k.key);
        if(!withoutClaim && !claimFor(plugin, to, trusted)) throw std::invalid_argument(plugin.id + " " + to + " does not say it opens sounds saved with " + plugin.version + ": use it without the claim");
        // The nodes: those asked, else every node playing the plugin at that version (a stand-in plays nothing).
        auto playing = [&](NodeId n) {
            const auto& h = graph_.host(n);
            const std::string format = fs::path(h.descriptor().path).extension() == ".lv2" ? "lv2" : "clap";
            return format == plugin.format && h.descriptor().id == plugin.id && h.descriptor().version == plugin.version && h.failure().empty();
        };
        std::vector<NodeId> nodes;
        for(const auto n : asked)
        {
            if(!nodes_.count(n)) throw std::invalid_argument("Unknown node");
            if(!playing(n)) throw std::invalid_argument(nodes_.at(n) + " does not play " + plugin.id + " " + plugin.version);
            nodes.push_back(n);
        }
        if(asked.empty()) for(const auto& [n, name] : nodes_) if(playing(n)) nodes.push_back(n);
        if(nodes.empty()) throw std::invalid_argument("the open Performance does not play " + plugin.id + " " + plugin.version);
        // Each node's state, saved now, opened by the new build before anything changes.
        Target t; t.plugin = plugin; t.to = to; t.rate = sampleRate_; t.frames = maxFrames_; t.worker = isolatedWorker();
        state_files::TemporaryDirectory saved(dataHome());
        VersionUsed out;
        for(const auto n : nodes)
        {
            auto& h = graph_.host(n);
            const auto& name = nodes_.at(n);
            if(dynamic_cast<const SoundContext*>(&h)) throw std::invalid_argument(name + " shares an instance with other parts; move the Performance from the library instead");
            const auto dir = saved.path / std::to_string(n);
            h.saveSnapshot(dir.string());
            auto fresh = oscairn::open(build, plugin.id, to, t, assets_);
            auto same = [](const std::vector<kick::clap::Port>& a, const std::vector<kick::clap::Port>& b) {
                return a.size() == b.size() && std::equal(a.begin(), a.end(), b.begin(), [](const auto& x, const auto& y) { return x.id == y.id && x.channels == y.channels && x.flags == y.flags; });
            };
            if(!same(h.inputPorts(), fresh->inputPorts()) || !same(h.outputPorts(), fresh->outputPorts()) || h.noteInputPorts() != fresh->noteInputPorts() || h.noteOutputPorts() != fresh->noteOutputPorts())
                throw std::runtime_error(name + ": its audio or note ports are not the same in " + to);
            try { fresh->loadSnapshot(dir.string()); }
            catch(const std::exception& e) { throw std::runtime_error(name + ": its state does not open in " + to + " (" + e.what() + ")"); }
            // Compared with what the build playing now reads from the same state apart from the graph, as a library move
            // compares them: the playing host's own values hold what it was last told, not what a read-only parameter reads.
            auto now = oscairn::open(h.descriptor().path, plugin.id, plugin.version, t, assets_);
            now->loadSnapshot(dir.string());
            const auto before = values(*now.host), after = values(*fresh.host);
            for(const auto& [id, v] : before)
            {
                const auto a = after.find(id);
                if(a == after.end()) out.changes.push_back({name, v.first, id, v.second, std::nullopt});
                else if(std::abs(a->second.second - v.second) > 1e-9) out.changes.push_back({name, v.first, id, v.second, a->second.second});
            }
            for(const auto& [id, v] : after) if(!before.count(id)) out.changes.push_back({name, v.first, id, std::nullopt, v.second});
        }
        // Then each replaced by the new build with that state, all or none: one step, the old instances held by it for undo.
        record(); // an edit waiting for its step comes first
        const auto before = std::make_shared<const Model>(model()); // holds the old instances
        try
        {
            for(const auto n : nodes)
            {
                replace(n, build.string(), plugin.id);
                graph_.host(n).loadSnapshot((saved.path / std::to_string(n)).string());
            }
        }
        catch(...)
        {
            apply(model(), *before); // the nodes replaced so far get their old instances back, as they were
            throw;
        }
        record();
        out.revision = graphState().revision;
        return out;
    }
}
