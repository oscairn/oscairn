// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0043 step 2: library items moved to a newer installed version of a plugin, only when asked.
// The versioned fixture at 1.0.0 and 2.0.0 (2.0.0 adds Drive, reading 1.0.0's state as it
// played), each in a plugin package signed by one publisher, 2.0.0 claiming to open 1.0.0's
// state: the plan opens every item with 2.0.0 and lists Drive as a setting that reads differently;
// the move writes the version, the lock and stateSavedBy, leaves nodes/ byte for byte, and the
// items open with 2.0.0 and sound as before; Undo restores the documents exactly, leaving one
// saved since; a locked item is listed and untouched; a 2.0.0 that refuses 1.0.0's state, and one
// with other ports, refuse the item and change nothing; without the claim (none, or another
// publisher's) items are noClaim and only one moves, with withoutClaim; an item whose 1.0.0 is not
// installed opens as a stand-in, and moved it plays.
// Step 3, the open Performance: one part of two moved (parts.useVersion) plays 2.0.0 beside the
// other's 1.0.0 (mixedVersions), the other moved too aligns them, undo puts each back with its
// state, and saving writes no stateSavedBy; a 2.0.0 refusing the state changes nothing; two parts
// sharing one instance of the Basic Synth move together onto one instance of its next version.
//   oscairn-version-moves-test VERSIONED-1 VERSIONED-2 STRICT-2 MONO-2 OSCAIRN-SIGN BASIC-SYNTH BASIC-SYNTH-0.3.0
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "../../plugin-host/src/StateFiles.h"
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <sstream>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    const std::string id = "org.oscairn.test.versioned";
    std::string read(const fs::path& f) { std::ifstream in(f, std::ios::binary); std::ostringstream out; out << in.rdbuf(); return out.str(); }
    std::string shell(const fs::path& p) { std::string out = "'"; for(char c : p.string()) out += c == '\'' ? std::string("'\\''") : std::string(1, c); return out + "'"; }
    int run(const std::string& command) { return std::system((command + " >/dev/null").c_str()); }
    void device(const fs::path& home) { fs::create_directories(home); ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1); }
    std::string tool;
    fs::path temp;
    // A plugin package of one build at a version, signed by `key` (build and whole), with options (a claim).
    fs::path package(const std::string& build, const std::string& version, const std::string& key, const std::string& name, const std::string& options = {}, const std::string& id = ::id)
    {
        const auto folder = temp / ("build-" + name);
        fs::create_directories(folder);
        const auto copy = folder / "Plugin.clap";
        fs::copy_file(build, copy, fs::copy_options::overwrite_existing);
        fs::remove(copy.string() + ".sig");
        const auto keyFile = temp / (key + ".key");
        if(!fs::exists(keyFile)) run(tool + " key " + shell(keyFile));
        check(run(tool + " build " + shell(keyFile) + " clap " + id + " " + version + " " + platform() + " " + shell(copy)) == 0, "the build is signed");
        const auto archive = temp / (name + ".olib");
        fs::remove(archive);
        check(run(tool + " plugin-package --license MIT --key " + shell(keyFile) + " " + options + " " + shell(archive) + " clap " + id + " " + version + " Versioned " + platform() + "=" + shell(copy)) == 0, "packed " + name);
        return archive;
    }
    std::string claims(const std::string& from, const std::string& plugin = id) { return "--opens-state-from " + plugin + " " + from + " --sound-changes " + plugin + " 'Drive added.'"; }
    fs::path stored(const std::string& version, const std::string& plugin = id) { for(const auto& f : pluginStoreFolders()) if(f.parent_path().filename() == plugin + "@" + version) return f / "Plugin.clap"; return {}; }
    // What one block plays with a key down, the left channel's first sample.
    double level(Session& s)
    {
        std::vector<float> l(64), r(64);
        float* o[2]{l.data(), r.data()};
        const std::vector<kick::clap::Event> on{kick::clap::Event::note(true, 60, .8, 0)}, off{kick::clap::Event::note(false, 60, 0, 0)};
        s.service(); s.process(nullptr, 0, o, 2, 64, on);
        s.service(); s.process(nullptr, 0, o, 2, 64);
        const double out = l[10];
        s.service(); s.process(nullptr, 0, o, 2, 64, off);
        return out;
    }
    Control::VersionJob finish(Session& s, uint64_t job)
    {
        for(int i = 0; i < 1000 && s.versionsJob(job).state == "running"; ++i) s.service();
        return s.versionsJob(job);
    }
    const Control::VersionItem* item(const Control::VersionJob& j, const std::string& name) { for(const auto& i : j.items) if(i.name == name) return &i; return nullptr; }
    std::string ids(Session& s, const std::string& name) { for(const auto& i : s.libraryQuery({}).items) if(i.name == name && i.library.empty()) return i.id; return {}; }
    fs::path path(Session& s, const std::string& id) { Control::LibraryQuery q; q.id = id; const auto r = s.libraryQuery(q); return r.items.empty() ? fs::path() : fs::path(r.items[0].path); }
    // Every file under nodes/, path and bytes.
    std::map<std::string, std::string> nodes(const fs::path& item)
    {
        std::map<std::string, std::string> out;
        for(const auto& f : fs::recursive_directory_iterator(item / "nodes")) if(f.is_regular_file()) out[fs::relative(f.path(), item).string()] = read(f.path());
        return out;
    }
    // A Performance of two parts on 1.0.0 (Level .3 and .7), saved as Set, its first part as the Sound Warm, and a copy as Copy.
    double makeItems(Session& s)
    {
        const auto v1 = stored("1.0.0");
        const auto a = s.add("Lead", v1.string(), id), b = s.add("Pad", v1.string(), id);
        const auto lead = s.addPart("Lead"), pad = s.addPart("Pad");
        s.insertIntoPart(lead, 0, a); s.insertIntoPart(pad, 0, b); s.commit();
        s.setParameter(a, 1, .3); s.setParameter(b, 1, .7);
        const auto played = level(s);
        s.savePartAsSound(lead, "Warm");
        s.savePerformanceToLibrary("Set");
        s.savePerformanceToLibrary("Copy", true);
        return played;
    }
    const Control::PluginRef old{"clap", id, "1.0.0"};
}

int main(int argc, char** argv)
{
    if(argc < 8) { std::fprintf(stderr, "usage: %s VERSIONED-1 VERSIONED-2 STRICT-2 MONO-2 OSCAIRN-SIGN BASIC-SYNTH BASIC-SYNTH-0.3.0\n", argv[0]); return 2; }
    const std::string synth = argv[6], synthNext = argv[7];
    const std::string v1 = argv[1], v2 = argv[2], strict = argv[3], mono = argv[4];
    tool = shell(argv[5]);
    try
    {
        state_files::TemporaryDirectory scratch;
        temp = scratch.path;
        const auto one = package(v1, "1.0.0", "publisher", "v1");
        const auto two = package(v2, "2.0.0", "publisher", "v2", claims("1.0.0"));
        // Device A: the claim honored; plan, move, open, undo.
        {
            device(temp / "a");
            Session s(48000, 64);
            s.installArchive(one.string(), true, {}, true);
            const auto played = makeItems(s);
            check(played > 0, "1.0.0 plays");
            s.installArchive(two.string(), true, {}, true);
            const auto set = ids(s, "Set"), warm = ids(s, "Warm"), copy = ids(s, "Copy");
            const auto setPath = path(s, set);
            const auto before = read(setPath / "performance.json"), warmBefore = read(path(s, warm) / "sound.json");
            const auto files = nodes(setPath);
            bool refused = false; try { s.versionsPlan(old, "0.9.0", {{}, {}, {}, true}); } catch(const std::invalid_argument&) { refused = true; }
            check(refused, "an older version is refused");
            refused = false; try { s.versionsPlan(old, "3.0.0", {{}, {}, {}, true}); } catch(const std::invalid_argument&) { refused = true; }
            check(refused, "a version not installed is refused");
            const auto plan = finish(s, s.versionsPlan(old, "2.0.0", {{}, {}, {}, true}));
            check(plan.state == "done" && plan.items.size() == 3 && plan.claim && plan.claim->opensStateFrom == "1.0.0" && plan.claim->soundChanges == "Drive added.", "the plan lists each item with the claim");
            const auto* s1 = item(plan, "Set");
            check(s1 && s1->status == "changed" && s1->nodes.size() == 2 && s1->changes.size() == 2 && s1->changes[0].name == "Drive" && !s1->changes[0].before && s1->changes[0].after == 0.0,
                  "Drive, new in 2.0.0, reads differently in each node; Level reads the same: " + (s1 ? s1->status + " " + s1->reason : std::string("missing")));
            check(read(setPath / "performance.json") == before && plan.move.empty(), "a plan changes nothing");
            refused = false; try { s.versionsMove(old, "2.0.0", {{}, {}, {}, true}, {}); } catch(const std::invalid_argument&) { refused = true; }
            check(refused, "a move names its items");
            const auto moved = finish(s, s.versionsMove(old, "2.0.0", {{}, {}, {}, true}, {set, warm, copy}));
            check(moved.state == "done" && moved.items.size() == 3 && std::all_of(moved.items.begin(), moved.items.end(), [](const auto& i) { return i.status == "moved"; }) && !moved.move.empty(),
                  "every item moves: " + moved.state + " " + moved.error);
            const auto after = read(setPath / "performance.json");
            check(after.find("\"version\":\"2.0.0\"") != std::string::npos && after.find("\"stateSavedBy\":\"1.0.0\"") != std::string::npos && after.find("\"version\":\"1.0.0\"") == std::string::npos,
                  "the document names 2.0.0, the state 1.0.0 saved, and no 1.0.0 in its lock");
            check(after.find("\"version\":\"2.0.0\",\"platform\":\"" + platform() + "\"") != std::string::npos, "the lock has 2.0.0's build");
            check(nodes(setPath) == files, "nodes/ is byte for byte as it was");
            const auto undone = s.versionsUndo(moved.move);
            check(undone.size() == 3 && std::all_of(undone.begin(), undone.end(), [](const auto& u) { return u.second == "putBack"; }) && read(setPath / "performance.json") == before &&
                  read(path(s, warm) / "sound.json") == warmBefore, "undo puts every document back exactly");
            refused = false; try { s.versionsUndo(moved.move); } catch(const std::invalid_argument&) { refused = true; }
            check(refused, "a move is undone once");
            const auto again = finish(s, s.versionsMove(old, "2.0.0", {{set, warm}, {}, {}, false}, {set, warm}));
            check(again.items.size() == 2 && again.items[0].status == "moved" && again.items[1].status == "moved", "moved again");
            // The moved Performance opens with 2.0.0, its saved state read, and sounds as before.
            s.loadPerformanceFromLibrary(set);
            const auto graph = s.graphState().topology.nodes;
            check(s.sessionStatus().failures.empty() && graph.size() == 2 && graph[0].plugin.version == "2.0.0" && graph[1].plugin.version == "2.0.0", "it opens with 2.0.0, no stand-in");
            check(std::abs(s.parameter(graph[0].id, 1) - .3) < 1e-9 && std::abs(level(s) - played) < 1e-6, "with its settings, sounding as before");
            s.savePerformanceToLibrary("Set");
            check(read(setPath / "performance.json").find("stateSavedBy") == std::string::npos, "saving drops stateSavedBy: the state is 2.0.0's now");
            const auto partly = s.versionsUndo(again.move);
            check(partly.size() == 2 && partly[0] == std::pair<std::string, std::string>{set, "savedSince"} && partly[1] == std::pair<std::string, std::string>{warm, "putBack"},
                  "undo leaves an item saved since and puts back the other");
            const auto kept = s.versionMoves();
            check(kept.size() == 2 && !kept[0].undoable && !kept[1].undoable && kept[0].to == "2.0.0", "the kept moves are listed");
            s.versionsForget(moved.move);
            check(s.versionMoves().size() == 1 && !fs::exists(temp / "a" / "Moves" / moved.move), "a forgotten move goes");
            // Locked: a library installed from an export of a 1.0.0 Performance.
            s.loadPerformanceFromLibrary(copy);
            const auto exported = temp / "Locked.olib";
            check(s.exportPerformance(exported.string()).empty(), "exported");
            s.deleteLibraryItem(copy); // one ID listed once, the user's own first: the locked copy alone
            s.installArchive(exported.string(), true, {}, true);
            std::string lockedId;
            for(const auto& i : s.libraryQuery({}).items) if(!i.library.empty()) lockedId = i.id;
            const auto lockedDoc = read(path(s, lockedId) / "performance.json");
            const auto locked = finish(s, s.versionsMove(old, "2.0.0", {{lockedId}, {}, {}, false}, {lockedId}));
            check(!lockedId.empty() && locked.items.size() == 1 && locked.items[0].status == "locked" && locked.items[0].locked && !locked.items[0].library.empty() && locked.move.empty(),
                  "a locked item is listed as locked");
            check(read(path(s, lockedId) / "performance.json") == lockedDoc, "and left untouched");
        }
        // Devices B and C: a 2.0.0 that refuses 1.0.0's state, and one with other ports, claiming it all the same.
        for(const auto& [build, why] : {std::pair{strict, std::string("state")}, std::pair{mono, std::string("port")}})
        {
            device(temp / ("b" + why));
            Session s(48000, 64);
            s.installArchive(one.string(), true, {}, true);
            makeItems(s);
            s.installArchive(package(build, "2.0.0", "publisher", "bad-" + why, claims("1.0.0")).string(), true, {}, true);
            const auto set = ids(s, "Set");
            const auto before = read(path(s, set) / "performance.json");
            const auto j = finish(s, s.versionsMove(old, "2.0.0", {{set}, {}, {}, false}, {set}));
            check(j.items.size() == 1 && j.items[0].status == "refused" && !j.items[0].reason.empty() && read(path(s, set) / "performance.json") == before && s.versionMoves().empty(),
                  "a 2.0.0 refusing the item (" + why + ") moves nothing: " + (j.items.empty() ? std::string() : j.items[0].reason));
            s.loadPerformanceFromLibrary(set);
            const auto graph = s.graphState().topology.nodes;
            bool refused = false; try { s.useVersion(old, "2.0.0"); } catch(const std::exception&) { refused = true; }
            check(refused && s.graphState().topology.nodes.at(0).plugin.version == "1.0.0" && !s.performanceInfo().unsaved,
                  "nor in the open Performance (" + why + ")");
        }
        // Device D: 2.0.0 claiming nothing, and device F: claimed by another publisher's key.
        for(const auto& [archive, what] : {std::pair{package(v2, "2.0.0", "publisher", "v2-plain"), std::string("no claim")}, std::pair{package(v2, "2.0.0", "other", "v2-other", claims("1.0.0")), std::string("another publisher's claim")}})
        {
            device(temp / ("d" + std::to_string(what.size())));
            Session s(48000, 64);
            s.installArchive(one.string(), true, {}, true);
            const auto played = makeItems(s);
            s.installArchive(archive.string(), true, {}, true);
            const auto set = ids(s, "Set"), warm = ids(s, "Warm");
            const auto plan = finish(s, s.versionsPlan(old, "2.0.0", {{}, {}, {}, true}));
            check(!plan.claim && plan.items.size() == 3 && std::all_of(plan.items.begin(), plan.items.end(), [](const auto& i) { return i.status == "noClaim"; }), what + ": every item is noClaim");
            const auto none = finish(s, s.versionsMove(old, "2.0.0", {{}, {}, {}, true}, {set, warm}));
            check(none.move.empty() && read(path(s, set) / "performance.json").find("2.0.0") == std::string::npos, what + ": moving them is refused");
            bool refused = false; try { s.versionsMove(old, "2.0.0", {{set, warm}, {}, {}, false}, {}, true); } catch(const std::invalid_argument&) { refused = true; }
            check(refused, what + ": without the claim, not two at once");
            const auto j = finish(s, s.versionsMove(old, "2.0.0", {{set}, {}, {}, false}, {}, true));
            check(j.items.size() == 1 && j.items[0].status == "moved", what + ": one item moves with withoutClaim");
            s.loadPerformanceFromLibrary(set);
            check(s.sessionStatus().failures.empty() && std::abs(level(s) - played) < 1e-6, what + ": and plays");
        }
        // Device E: 1.0.0 gone, 2.0.0 claiming it installed: the items open with a stand-in; the claim
        // cannot be checked against 1.0.0's publisher, so one moves at a time, and moved, it plays.
        {
            device(temp / "e");
            double played = 0;
            { Session s(48000, 64); s.installArchive(one.string(), true, {}, true); played = makeItems(s); }
            Session s(48000, 64);
            s.removePlugin("clap", id, "1.0.0");
            s.installArchive(two.string(), true, {}, true);
            const auto set = ids(s, "Set");
            s.loadPerformanceFromLibrary(set);
            check(s.sessionStatus().failures.size() == 2 && level(s) == 0, "1.0.0 not here: stand-ins, silent");
            const auto plan = finish(s, s.versionsPlan(old, "2.0.0", {{set}, {}, {}, false}));
            check(plan.items.size() == 1 && plan.items[0].status == "noClaim" && plan.items[0].reason.find("not compared") != std::string::npos, "offered without the claim: " + (plan.items.empty() ? std::string() : plan.items[0].status));
            const auto j = finish(s, s.versionsMove(old, "2.0.0", {{set}, {}, {}, false}, {}, true));
            check(j.items.size() == 1 && j.items[0].status == "moved", "moved");
            s.loadPerformanceFromLibrary(set);
            check(s.sessionStatus().failures.empty() && std::abs(level(s) - played) < 1e-6, "it plays, as it did with 1.0.0");
        }
        // Device G, step 3: the open Performance's parts.
        {
            device(temp / "g");
            Session s(48000, 64);
            s.installArchive(one.string(), true, {}, true);
            const auto played = makeItems(s);
            s.installArchive(two.string(), true, {}, true);
            s.loadPerformanceFromLibrary(ids(s, "Set"));
            const auto lead = s.nodeId("Lead"), pad = s.nodeId("Pad");
            auto version = [&](NodeId node) { for(const auto& n : s.graphState().topology.nodes) if(n.id == node) return n.plugin.version; return std::string(); };
            bool refused = false; try { s.useVersion(old, "2.0.0", {lead}, false); s.useVersion({"clap", id, "2.0.0"}, "1.0.0"); } catch(const std::invalid_argument&) { refused = true; }
            check(refused, "a move back to an older version is refused");
            s.undo();
            check(version(lead) == "1.0.0" && version(pad) == "1.0.0", "undone");
            const auto used = s.useVersion(old, "2.0.0", {lead});
            const auto mixed = s.sessionStatus().mixedVersions;
            check(used.nodes == std::vector<NodeId>{lead} && used.changes.size() == 1 && used.changes[0].name == "Drive" && !used.changes[0].before, "one part moved; Drive is new in it");
            check(version(lead) == "2.0.0" && version(pad) == "1.0.0" && s.nodeId("Lead") == lead && mixed.size() == 1 && mixed[0].id == id && mixed[0].versions == std::vector<std::string>{"1.0.0", "2.0.0"},
                  "two versions play at once, said in mixedVersions");
            check(std::abs(s.parameter(lead, 1) - .3) < 1e-9 && std::abs(level(s) - played) < 1e-6 && s.performanceInfo().unsaved, "with its state, sounding as before; an edit");
            s.useVersion(old, "2.0.0");
            check(version(pad) == "2.0.0" && s.sessionStatus().mixedVersions.empty() && std::abs(s.parameter(pad, 1) - .7) < 1e-9, "the other moved too: one version");
            s.undo();
            check(version(pad) == "1.0.0" && std::abs(s.parameter(pad, 1) - .7) < 1e-9, "undo puts 1.0.0 back with its state");
            s.undo();
            check(version(lead) == "1.0.0" && std::abs(s.parameter(lead, 1) - .3) < 1e-9 && std::abs(level(s) - played) < 1e-6, "and the first");
            s.redo(); s.redo();
            check(version(lead) == "2.0.0" && version(pad) == "2.0.0", "redo moves them again");
            s.savePerformanceToLibrary("Set");
            const auto saved = read(path(s, ids(s, "Set")) / "performance.json");
            check(saved.find("stateSavedBy") == std::string::npos && saved.find("\"version\":\"1.0.0\"") == std::string::npos, "saved with 2.0.0's own state, no stateSavedBy");
        }
        // Device H: two parts sharing one instance of the Basic Synth move together.
        {
            device(temp / "h");
            const auto basic = "org.oscairn.basic-synth";
            Session s(48000, 64);
            s.installArchive(package(synth, "0.2.0", "publisher", "synth", {}, basic).string(), true, {}, true);
            s.installArchive(package(synthNext, "0.3.0", "publisher", "synth-next", claims("0.2.0", basic), basic).string(), true, {}, true);
            const auto first = s.add("one", stored("0.2.0", basic).string(), basic);
            const auto second = s.shareInstrument("two", first);
            const auto a = s.addPart("A"), b = s.addPart("B");
            s.insertIntoPart(a, 0, first); s.insertIntoPart(b, 0, second); s.commit();
            s.setParameter(second, 1, .25);
            level(s);
            const auto before = s.parameter(second, 1);
            check(s.graphState().shared.size() == 2, "two parts share one instance");
            const auto used = s.useVersion({"clap", basic, "0.2.0"}, "0.3.0", {second});
            const auto g = s.graphState();
            check(used.nodes.size() == 2 && g.shared.size() == 2 && g.shared.at(first).engine == g.shared.at(second).engine && s.sessionStatus().mixedVersions.empty() &&
                  std::all_of(g.topology.nodes.begin(), g.topology.nodes.end(), [](const auto& n) { return n.plugin.version == "0.3.0"; }),
                  "naming one moves both, onto one instance of 0.3.0");
            check(std::abs(s.parameter(second, 1) - before) < 1e-9, "each keeps its sound's settings");
            s.undo();
            const auto back = s.graphState();
            check(back.shared.size() == 2 && std::all_of(back.topology.nodes.begin(), back.topology.nodes.end(), [](const auto& n) { return n.plugin.version == "0.2.0"; }), "undo puts the shared 0.2.0 instance back");
        }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); ++failures; }
    if(failures) std::fprintf(stderr, "%d failures\n", failures);
    else std::puts("version moves: ok");
    return failures ? 1 : 0;
}
