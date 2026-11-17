// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Moving library items to a newer installed version of a plugin (RFC 0043 step 2), only when the
// user asks. A plan opens each item's nodes of the plugin with the new version, as opening does,
// in a graph that never plays (Session::openNodes), one item each service(); a move then writes
// the new version, the store's build as the path hint, the lock and stateSavedBy into the item's
// document, leaving its saved state byte for byte, and keeps the document as it was under
// Moves/<move>/ in the data folder, so undoing it is putting one file back.
#include "oscairn/session.h"
#include "oscairn/assets.h"
#include "oscairn/library.h"
#include "oscairn/package.h"
#include "oscairn/setlist.h"
#include "JsonInput.h"
#include "PluginStore.h"
#include "../../plugin-host/src/StateFiles.h"
#include <chrono>
#include <cmath>
#include <ctime>
#include <memory>
#include <random>
#include <set>

namespace oscairn
{
    namespace
    {
        namespace fs = std::filesystem;
        using V = const json_value_s*;
        using Json = std::unique_ptr<json_value_s, decltype(&std::free)>;
        constexpr size_t maxItems = 10000;
        [[noreturn]] void bad(const std::string& why) { throw std::invalid_argument(why); }
        std::string str(V v) { if(!v || v->type != json_type_string) return {}; const auto* s = static_cast<const json_string_s*>(v->payload); return {s->string, s->string_size}; }
        std::string slurp(const fs::path& file) { const auto b = state_files::read(file, 4 * 1024 * 1024); return {b.begin(), b.end()}; }
        Json parse(const std::string& text)
        {
            if(!detail::jsonPreflight(text, 32).empty()) throw std::runtime_error("its document is not valid JSON");
            Json doc{json_parse(text.data(), text.size()), &std::free};
            if(!doc) throw std::runtime_error("its document is not valid JSON");
            return doc;
        }
        std::vector<V> list(V v) { std::vector<V> out; if(v && v->type == json_type_array) for(auto* e = static_cast<const json_array_s*>(v->payload)->start; e; e = e->next) out.push_back(e->value); return out; }
        std::string written(V v) { size_t n = 0; std::unique_ptr<char, decltype(&std::free)> s{static_cast<char*>(json_write_minified(v, &n)), &std::free}; if(!s) throw std::runtime_error("cannot write the document"); return {s.get(), n ? n - 1 : 0}; }
        std::string fresh() { std::random_device r; constexpr char hex[] = "0123456789abcdef"; std::string id; for(int i = 0; i < 16; ++i) id += hex[r() & 15]; return id; }
        std::string today() { const auto t = std::time(nullptr); std::tm tm{}; gmtime_r(&t, &tm); char s[16]; std::strftime(s, sizeof s, "%Y-%m-%d", &tm); return s; }
        fs::path moves() { return dataHome() / "Moves"; }
        // The keys of a document's nodes playing the plugin at that version.
        std::vector<std::string> pluginNodes(V doc, const Control::PluginRef& p)
        {
            std::vector<std::string> out;
            for(auto n : list(detail::member(doc, "nodes")))
                if(const V q = detail::member(n, "plugin"); str(detail::member(q, "format")) == p.format && str(detail::member(q, "id")) == p.id && str(detail::member(q, "version")) == p.version)
                    out.push_back(str(detail::member(n, "key")));
            return out;
        }
        // The document with its nodes of the plugin moved to `to`: the version, the store's build as
        // the path hint, stateSavedBy (the version whose state the node holds, kept from an earlier
        // move), and the lock's entries for the old version replaced by the new one's builds here.
        std::string moved(V doc, const Control::PluginRef& p, const std::string& to, const fs::path& build)
        {
            std::string out = "{";
            for(auto* m = static_cast<const json_object_s*>(doc->payload)->start; m; m = m->next)
            {
                const std::string name(m->name->string, m->name->string_size);
                out += (out.size() > 1 ? "," : "") + detail::quote(name) + ":";
                if(name == "nodes")
                {
                    std::string nodes;
                    for(auto n : list(m->value))
                    {
                        const V q = detail::member(n, "plugin");
                        const bool mine = str(detail::member(q, "format")) == p.format && str(detail::member(q, "id")) == p.id && str(detail::member(q, "version")) == p.version;
                        std::string node = written(n);
                        if(mine)
                        {
                            const auto savedBy = detail::member(q, "stateSavedBy") ? str(detail::member(q, "stateSavedBy")) : p.version;
                            node = "{";
                            for(auto* f = static_cast<const json_object_s*>(n->payload)->start; f; f = f->next)
                            {
                                const std::string field(f->name->string, f->name->string_size);
                                node += (node.size() > 1 ? "," : "") + detail::quote(field) + ":" +
                                        (field == "plugin" ? "{\"format\":" + detail::quote(p.format) + ",\"id\":" + detail::quote(p.id) + ",\"version\":" + detail::quote(to) +
                                                             ",\"pathHint\":" + detail::quote(build.string()) + ",\"stateSavedBy\":" + detail::quote(savedBy) + "}"
                                                           : written(f->value));
                            }
                            node += "}";
                        }
                        nodes += (nodes.empty() ? "" : ",") + node;
                    }
                    out += "[" + nodes + "]";
                }
                else if(name == "lock")
                {
                    std::string entries;
                    for(auto e : list(detail::member(m->value, "plugins")))
                        if(!(str(detail::member(e, "format")) == p.format && str(detail::member(e, "id")) == p.id && (str(detail::member(e, "version")) == p.version || str(detail::member(e, "version")) == to)))
                            entries += (entries.empty() ? "" : ",") + written(e);
                    for(const auto& [platform, b] : store::builds(p.format, p.id, to))
                        entries += (entries.empty() ? "" : ",") + std::string("{\"format\":") + detail::quote(p.format) + ",\"id\":" + detail::quote(p.id) + ",\"version\":" + detail::quote(to) +
                                   ",\"platform\":" + detail::quote(platform) + ",\"sha256\":" + detail::quote(bundleDigest(b)) + "}";
                    out += "{\"plugins\":[" + entries + "]}";
                }
                else out += written(m->value);
            }
            return out + "}\n";
        }
        std::string moveJson(const Control::VersionMove& m, const std::vector<std::pair<std::string, std::string>>& items, bool undone)
        {
            std::string list;
            for(const auto& [id, revision] : items) list += (list.empty() ? "" : ",") + std::string("{\"id\":") + detail::quote(id) + ",\"revision\":" + detail::quote(revision) + "}";
            return "{\"format\":\"oscairn.move\",\"version\":1,\"plugin\":{\"format\":" + detail::quote(m.plugin.format) + ",\"id\":" + detail::quote(m.plugin.id) + ",\"version\":" + detail::quote(m.plugin.version) +
                   "},\"to\":" + detail::quote(m.to) + ",\"date\":" + detail::quote(m.date) + ",\"undone\":" + (undone ? "true" : "false") + ",\"items\":[" + list + "]}\n";
        }
        struct Kept { Control::VersionMove move; std::vector<std::pair<std::string, std::string>> items; bool undone = false; };
        Kept readMove(const std::string& move)
        {
            if(move.size() != 16 || move.find_first_not_of("0123456789abcdef") != std::string::npos) bad("no such move");
            std::error_code e;
            if(!fs::is_regular_file(moves() / move / "move.json", e)) bad("no such move");
            const auto doc = parse(slurp(moves() / move / "move.json"));
            Kept k;
            const V p = detail::member(doc.get(), "plugin");
            k.move = {move, str(detail::member(doc.get(), "date")), {str(detail::member(p, "format")), str(detail::member(p, "id")), str(detail::member(p, "version"))}, str(detail::member(doc.get(), "to")), {}, false};
            k.undone = detail::member(doc.get(), "undone") && detail::member(doc.get(), "undone")->type == json_type_true;
            for(auto i : list(detail::member(doc.get(), "items"))) { k.items.push_back({str(detail::member(i, "id")), str(detail::member(i, "revision"))}); k.move.items.push_back(k.items.back().first); }
            k.move.undoable = !k.undone && !k.items.empty();
            return k;
        }
    }

    struct Session::VersionWork
    {
        VersionJob job;
        PluginRef plugin;
        std::string to;
        fs::path build;            // the new version's build here
        bool move = false, withoutClaim = false;
        std::vector<std::string> queue; // the items still to do, in order
        std::set<std::string> chosen;   // a move's items
        std::vector<std::pair<std::string, std::string>> written; // a move's items moved, by ID and the revision it wrote
    };

    uint64_t Session::versionsPlan(const PluginRef& plugin, const std::string& to, const VersionScope& scope) { return startVersions(plugin, to, scope, {}, false, false); }

    uint64_t Session::versionsMove(const PluginRef& plugin, const std::string& to, const VersionScope& scope, const std::vector<std::string>& items, bool withoutClaim)
    {
        if(items.empty() && !withoutClaim) bad("name the items to move, from the plan");
        return startVersions(plugin, to, scope, items, withoutClaim, true);
    }

    uint64_t Session::startVersions(const PluginRef& plugin, const std::string& to, const VersionScope& scope, const std::vector<std::string>& items, bool withoutClaim, bool move)
    {
        if(versionWork_ && versionWork_->job.state == "running") bad("busy: a plan or move is running");
        if((plugin.format != "clap" && plugin.format != "lv2") || plugin.id.empty() || plugin.version.empty()) bad("name the plugin by format, ID and version");
        if(!assets::older(plugin.version, to)) bad("only a newer version than " + plugin.version + " can be moved to; going back is undoing the move");
        if(withoutClaim && (scope.items.size() != 1 || items.size() > 1)) bad("without the publisher's claim, one item moves at a time");
        auto w = std::make_shared<VersionWork>();
        w->plugin = plugin; w->to = to; w->build = store::build(plugin.format, plugin.id, to);
        if(w->build.empty()) bad(plugin.id + " " + to + " is not installed here");
        // The claim (RFC 0043): the new version opens this version's state, made by the publisher whose key signed the old build here.
        const auto target = store::identity(plugin.format, plugin.id, to);
        if(!target.opensStateFrom.empty() && !assets::older(plugin.version, target.opensStateFrom))
            if(const auto old = store::build(plugin.format, plugin.id, plugin.version); !old.empty())
            {
                const auto signatures = store::signatures(old, plugin.format, plugin.id, plugin.version, platform());
                if(std::any_of(signatures.begin(), signatures.end(), [&](const auto& s) { return s.key == target.claimKey; })) w->job.claim = VersionClaim{target.opensStateFrom, target.soundChanges, target.claimKey, false};
            }
        // The items: by ID, the user's own copied from a library, a Setlist's Performances, or every one using it.
        auto& l = library();
        std::vector<std::string> ids;
        if(!scope.items.empty()) ids = scope.items;
        else if(!scope.library.empty()) { for(const auto& i : l.items()) if(i.library.empty() && i.origin.library == scope.library) ids.push_back(i.id); }
        else if(!scope.setlist.empty()) ids = setlistPerformances(setlist(scope.setlist));
        else if(scope.all) { for(const auto& i : l.items()) if(i.kind == "sound" || i.kind == "performance") ids.push_back(i.id); }
        else bad("a scope: items, a library, a Setlist or all");
        if(ids.size() > maxItems) bad("at most 10,000 items at a time");
        w->queue = ids; w->move = move; w->withoutClaim = withoutClaim;
        w->chosen = withoutClaim ? std::set<std::string>{scope.items.front()} : std::set<std::string>{items.begin(), items.end()};
        w->job.id = ++versionJobs_; w->job.state = "running"; if(move) w->job.move = fresh();
        versionWork_ = w;
        return w->job.id;
    }

    Control::VersionJob Session::versionsJob(uint64_t job)
    {
        if(!versionWork_ || versionWork_->job.id != job) bad("no such job");
        return versionWork_->job;
    }

    void Session::advanceVersions()
    {
        if(!versionWork_ || versionWork_->job.state != "running") return;
        auto& w = *versionWork_;
        if(w.queue.empty())
        {
            if(w.move && !w.written.empty())
            {
                try { const auto text = moveJson({w.job.move, today(), w.plugin, w.to, {}, true}, w.written, false); writeAtomically(moves() / w.job.move / "move.json", text); }
                catch(const std::exception& e) { w.job.error = e.what(); }
                library().rescan();
            }
            if(!w.move || w.written.empty()) w.job.move.clear();
            w.job.state = w.job.error.empty() ? "done" : "failed";
            return;
        }
        const auto id = w.queue.front(); w.queue.erase(w.queue.begin());
        if(w.move && !w.chosen.count(id)) return; // a move does the items it names
        VersionItem out; out.id = id;
        try
        {
            const auto* item = library().find(id);
            if(!item) bad("not in the library");
            out.name = item->name; out.kind = item->kind; out.library = item->library;
            if(item->kind != "sound" && item->kind != "performance") bad("not a Sound or a Performance");
            const auto document = fs::path(item->path) / (item->kind == "sound" ? "sound.json" : "performance.json");
            const auto text = slurp(document);
            const auto doc = parse(text);
            out.nodes = pluginNodes(doc.get(), w.plugin);
            if(out.nodes.empty()) bad("it does not use " + w.plugin.id + " " + w.plugin.version);
            out.locked = !item->library.empty();
            if(out.locked)
            {
                std::string name = item->library;
                for(const auto& x : libraries()) if(x.id == item->library) name = x.name;
                out.status = "locked"; out.reason = "it is in the locked library " + name + ": its publisher moves it in a new version of the library";
            }
            else if(item->readOnly || item->inPlace) { out.status = "refused"; out.reason = "it is read-only here"; }
            else
            {
                // Opened with the new version; the old one's values to compare, where it is here.
                const auto after = openNodes(item->path, document.filename().string(), w.plugin, w.build.string(), w.to);
                std::map<std::string, std::vector<std::tuple<uint32_t, std::string, double>>> before;
                if(const auto old = store::build(w.plugin.format, w.plugin.id, w.plugin.version); !old.empty())
                    try { before = openNodes(item->path, document.filename().string(), w.plugin, old.string(), w.plugin.version); } catch(const std::exception&) {}
                else out.reason = "its settings are not compared: " + w.plugin.version + " is not here";
                if(!before.empty())
                    for(const auto& [key, values] : after)
                    {
                        const auto& was = before[key];
                        auto find = [](const auto& list, uint32_t param) { return std::find_if(list.begin(), list.end(), [&](const auto& v) { return std::get<0>(v) == param; }); };
                        for(const auto& [param, name, value] : was)
                            if(const auto now = find(values, param); now == values.end()) out.changes.push_back({key, name, param, value, std::nullopt});
                            else if(std::fabs(std::get<2>(*now) - value) > 1e-9) out.changes.push_back({key, name, param, value, std::get<2>(*now)});
                        for(const auto& [param, name, value] : values) if(find(was, param) == was.end()) out.changes.push_back({key, name, param, std::nullopt, value});
                    }
                out.status = !w.job.claim ? "noClaim" : out.changes.empty() ? "movable" : "changed";
                const bool go = w.move && (w.job.claim || w.withoutClaim);
                if(go)
                {
                    // The document as it was, kept first; then the moved one, with a new revision.
                    const auto kept = moves() / w.job.move / id;
                    fs::create_directories(kept);
                    state_files::write(kept / document.filename(), text.data(), text.size());
                    writeAtomically(document, moved(doc.get(), w.plugin, w.to, w.build));
                    auto info = readInfo(document, item->kind == "sound" ? "oscairn.sound" : "oscairn.performance");
                    info.parent = info.revision; info.revision = fresh(); info.hash = contentHash(document);
                    writeInfo(document, info);
                    w.written.push_back({id, info.revision});
                    out.status = "moved";
                }
            }
        }
        catch(const std::exception& e) { out.status = "refused"; out.reason = e.what(); }
        w.job.items.push_back(std::move(out));
    }

    std::vector<Control::VersionMove> Session::versionMoves()
    {
        std::vector<VersionMove> out;
        std::error_code e;
        for(const auto& entry : fs::directory_iterator(moves(), e))
            try { out.push_back(readMove(entry.path().filename().string()).move); } catch(const std::exception&) {}
        std::sort(out.begin(), out.end(), [](const auto& a, const auto& b) { return a.date > b.date; });
        return out;
    }

    std::vector<std::pair<std::string, std::string>> Session::versionsUndo(const std::string& move)
    {
        auto k = readMove(move);
        if(k.undone) bad("that move was undone already");
        std::vector<std::pair<std::string, std::string>> out;
        for(const auto& [id, revision] : k.items)
        {
            const auto* item = library().find(id);
            const auto document = item ? fs::path(item->path) / (item->kind == "sound" ? "sound.json" : "performance.json") : fs::path();
            std::error_code e;
            const auto kept = moves() / move / id / document.filename();
            if(!item || !fs::is_regular_file(kept, e) || readInfo(document, item->kind == "sound" ? "oscairn.sound" : "oscairn.performance").revision != revision) { out.push_back({id, "savedSince"}); continue; }
            writeAtomically(document, slurp(kept));
            out.push_back({id, "putBack"});
        }
        writeAtomically(moves() / move / "move.json", moveJson(k.move, k.items, true));
        library().rescan();
        return out;
    }

    void Session::versionsForget(const std::string& move)
    {
        readMove(move);
        fs::remove_all(moves() / move);
    }
}
