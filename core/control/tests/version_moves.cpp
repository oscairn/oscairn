// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Moving library items to a newer plugin version (RFC 0043 step 2), with the test synth at version
// 1 and builds of it at version 2 (kick-clap-fixture-v2*), each installed from a plugin package:
//  - the plan changes nothing and lists each item, with the publisher's claim;
//  - a move writes the version, the path and the lock, saves the state again as version 2 writes it,
//    keeps the item as it was under Moves/, and the item opens with version 2, not a stand-in;
//  - undo puts the item back byte for byte, and leaves an item saved since the move;
//  - a locked library's item is listed and untouched;
//  - a version 2 that refuses the state, or has other ports, refuses that item and moves the rest;
//  - a parameter version 2 reads differently is listed, and the state written is version 2's;
//  - without the claim, more than one item is refused and one moves with withoutClaim;
//  - at instrument, a move is refused while the transport runs.
//   oscairn-version-moves-test FIXTURE-V1 FIXTURE-V2 FIXTURE-V2-CHANGED FIXTURE-V2-PORTS OSCAIRN-SIGN [WORKER]
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "oscairn/assets.h"
#include "../../plugin-host/src/StateFiles.h"
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstring>
#include <functional>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <map>
#include <sstream>
#include <thread>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    const std::string id = "org.kick.test.synth";
    std::string sign;
    fs::path temp;

    std::string shell(const fs::path& p) { return "'" + p.string() + "'"; }
    int run(const std::string& command) { return std::system((command + " >/dev/null 2>&1").c_str()); }
    std::string slurp(const fs::path& f) { std::ifstream in(f, std::ios::binary); std::ostringstream out; out << in.rdbuf(); return out.str(); }
    // Every file in a folder by its path, with its bytes.
    std::map<std::string, std::string> tree(const fs::path& folder)
    {
        std::map<std::string, std::string> out;
        std::error_code missing;
        for(auto e = fs::recursive_directory_iterator(folder, missing); !missing && e != fs::recursive_directory_iterator(); ++e) if(e->is_regular_file()) out[fs::relative(e->path(), folder).generic_string()] = slurp(e->path());
        return out;
    }
    void home(const std::string& name) { fs::create_directories(temp / name); ::setenv("OSCAIRN_DATA_HOME", (temp / name).c_str(), 1); }
    // A build of the synth at `version`, signed, packed with or without the claim to open version 1's state, installed.
    void install(Session& s, const char* fixture, const std::string& version, bool claim)
    {
        const auto folder = temp / ("build-" + version + "-" + std::to_string(std::rand()));
        fs::create_directories(folder);
        const auto build = folder / "Synth.clap";
        fs::copy_file(fixture, build);
        check(run(sign + " build " + shell(temp / "author.key") + " clap " + id + " " + version + " " + platform() + " " + shell(build)) == 0, "the build is signed");
        const auto archive = folder / "Synth.olib";
        check(run(sign + " plugin-package --license MIT --key " + shell(temp / "author.key") + (claim ? " --opens-state-from " + id + " 1" : std::string()) + " " + shell(archive) +
                  " clap " + id + " " + version + " Synth " + platform() + "=" + shell(build)) == 0, "the package is packed");
        s.installArchive(archive.string(), false, {}, true);
        // A claim counts only under a key the trust store trusts for plugins, as the publisher's would be.
        if(claim) for(const auto& p : s.installedPlugins()) if(p.version == version && !p.claimKey.empty()) s.setTrustedKey({"", p.claimKey, "Author", {"plugins"}});
    }
    fs::path stored(const std::string& version) { return fs::path(std::getenv("OSCAIRN_DATA_HOME")) / "Plugins" / "clap" / (id + "@" + version) / platform() / "Synth.clap"; }
    struct Saved { std::string performance, sound; };
    // A part playing version 1 at gain .8, saved as a Performance and a Sound.
    Saved save(Session& s, const std::string& name)
    {
        Saved out;
        {
            Session t(rate, frames);
            const auto n = t.add("synth", stored("1").string(), id);
            const auto p = t.addPart(name); t.insertIntoPart(p, 0, n); t.commit();
            t.setParameter(n, 7, .8); t.service();
            out.sound = t.savePartAsSound(p, name + " Sound", true).id;
            out.performance = t.savePerformanceToLibrary(name, true);
        }
        s.libraryRescan();
        return out;
    }
    Control::VersionJob wait(Session& s, uint64_t job)
    {
        for(int i = 0; i < 3000; ++i)
        {
            auto j = s.versionJob(job);
            if(j.state != "running") return j;
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
        check(false, "the job ends");
        return {};
    }
    const Control::VersionItem* item(const Control::VersionJob& j, const std::string& itemId)
    {
        for(const auto& i : j.items) if(i.id == itemId) return &i;
        return nullptr;
    }
    std::string status(const Control::VersionJob& j, const std::string& itemId) { const auto* i = item(j, itemId); return i ? i->status + (i->reason.empty() ? "" : " (" + i->reason + ")") : "absent"; }
    fs::path pathOf(Session& s, const std::string& itemId) { Control::LibraryQuery q; q.id = itemId; const auto r = s.libraryQuery(q); return r.items.empty() ? fs::path() : fs::path(r.items[0].path); }
    std::string refusal(const std::function<void()>& call) { try { call(); } catch(const std::exception& e) { return e.what(); } return {}; }
    const Control::PluginVersion v1{"clap", id, "1"};
}

int main(int argc, char** argv)
{
    if(argc < 6) return 2;
    const char *fixture = argv[1], *v2 = argv[2], *changed = argv[3], *ports = argv[4];
    sign = shell(argv[5]);
    if(argc > 6) ::setenv("OSCAIRN_PLUGIN_WORKER", argv[6], 1);
    state_files::TemporaryDirectory scratch;
    temp = scratch.path;
    check(run(sign + " key " + shell(temp / "author.key")) == 0, "a key is made");

    // Version 2, claiming version 1's state.
    {
        home("a");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", true);
        const auto a = save(s, "Alpha"), b = save(s, "Beta"), spare = save(s, "Spare");
        const auto before = tree(pathOf(s, a.performance));
        // The plan: every item using version 1, the claim, nothing changed.
        auto plan = wait(s, s.planVersionMove(v1, "2", {.all = true}));
        check(plan.state == "done" && plan.items.size() == 6 && status(plan, a.performance) == "movable" && status(plan, a.sound) == "movable" && item(plan, a.performance)->nodes == 1,
              "the plan lists each item using version 1 as movable: " + status(plan, a.performance) + ", " + status(plan, a.sound) + ", " + std::to_string(plan.items.size()) + " items");
        check(plan.claim && plan.claim->opensStateFrom == "1" && plan.move.empty(), "with the publisher's claim");
        check(tree(pathOf(s, a.performance)) == before && !fs::exists(fs::path(std::getenv("OSCAIRN_DATA_HOME")) / "Moves"), "the plan changes nothing");
        // The move.
        auto moved = wait(s, s.moveVersions(v1, "2", {a.performance, a.sound}));
        check(moved.state == "done" && status(moved, a.performance) == "moved" && status(moved, a.sound) == "moved" && !moved.move.empty(),
              "both items move: " + status(moved, a.performance) + ", " + status(moved, a.sound));
        const auto doc = slurp(pathOf(s, a.performance) / "performance.json");
        check(doc.find("\"version\":\"2\",\"pathHint\":\"" + stored("2").string() + "\"") != std::string::npos, "the node names version 2 and its build here");
        check(doc.find("\"id\":\"" + id + "\",\"version\":\"2\",\"platform\":\"" + platform() + "\",\"sha256\":\"" + bundleDigest(stored("2")) + "\"") != std::string::npos &&
                  doc.find("\"id\":\"" + id + "\",\"version\":\"1\",\"platform\"") == std::string::npos,
              "the lock holds version 2's build by its digest, version 1's no more");
        check(tree(fs::path(std::getenv("OSCAIRN_DATA_HOME")) / "Moves" / moved.move / a.performance) == before, "the item as it was is kept for undo, byte for byte");
        const auto moves = s.versionMoves();
        check(moves.size() == 1 && moves[0].move == moved.move && moves[0].to == "2" && moves[0].items.size() == 2 && moves[0].undoable, "the move is kept");
        s.loadPerformanceFromLibrary(a.performance);
        const auto node = s.graphState().topology.nodes.at(0);
        check(s.sessionStatus().failures.empty() && node.plugin.version == "2" && std::abs(s.parameter(node.id, 7) - .8) < 1e-9, "the item opens with version 2, its setting kept");
        // The open Performance's own item is not moved: saving it would write the old versions over the move.
        auto open = wait(s, s.moveVersions(v1, "2", {a.performance}));
        check(status(open, a.performance).find("is the open Performance") != std::string::npos && open.move.empty(), "the open Performance's item is refused: " + status(open, a.performance));
        s.loadPerformanceFromLibrary(spare.performance);
        // Undo puts it back exactly; then one saved since the move is left.
        const auto undone = s.undoVersionMove(moved.move);
        check(undone.size() == 2 && !undone[0].savedSince && !undone[1].savedSince && tree(pathOf(s, a.performance)) == before && s.versionMoves().empty(), "undo restores the items exactly");
        moved = wait(s, s.moveVersions(v1, "2", {a.performance, b.performance}));
        s.loadPerformanceFromLibrary(a.performance);
        s.savePerformanceToLibrary("Alpha");
        s.loadPerformanceFromLibrary(spare.performance);
        const auto partly = s.undoVersionMove(moved.move);
        check(partly.size() == 2 && std::count_if(partly.begin(), partly.end(), [&](const auto& u) { return u.id == a.performance && u.savedSince; }) == 1 &&
                  slurp(pathOf(s, a.performance) / "performance.json").find("\"version\":\"2\",\"pathHint\"") != std::string::npos &&
                  slurp(pathOf(s, b.performance) / "performance.json").find("\"version\":\"1\",\"pathHint\"") != std::string::npos,
              "undo leaves an item saved since the move, and puts back the other");
        check(s.versionMoves().size() == 1 && !s.versionMoves()[0].undoable, "what is left of the move is listed, not undoable");
        s.forgetVersionMove(moved.move);
        check(s.versionMoves().empty(), "and forgotten");
        // A state version 2 refuses (once): that item is refused, the rest moves.
        const auto refuse = temp / "refuse";
        std::ofstream(refuse) << "1";
        ::setenv("KICK_FIXTURE_REFUSE_STATE", refuse.c_str(), 1);
        const auto c = save(s, "Gamma");
        auto some = wait(s, s.moveVersions(v1, "2", {b.performance, c.performance}));
        ::unsetenv("KICK_FIXTURE_REFUSE_STATE");
        const auto first = std::min(b.performance, c.performance), second = std::max(b.performance, c.performance);
        check(status(some, first).rfind("refused", 0) == 0 && status(some, first).find("does not open in 2") != std::string::npos && status(some, second) == "moved",
              "a state version 2 refuses refuses that item and moves the rest: " + status(some, first) + ", " + status(some, second));
        check(slurp(pathOf(s, first) / "performance.json").find("\"version\":\"1\",\"pathHint\"") != std::string::npos, "the refused item is untouched");
        // A locked library's item: listed, never moved.
        const auto archive = temp / "Locked.olib";
        s.exportLibraryItem(b.sound, archive.string());
        s.deleteLibraryItem(b.sound);
        s.installArchive(archive.string(), true, {}, true);
        auto locked = wait(s, s.planVersionMove(v1, "2", {.items = {b.sound}}));
        check(status(locked, b.sound) == "locked" && item(locked, b.sound)->locked, "a locked library's item is listed as locked: " + status(locked, b.sound));
        check(refusal([&] { s.moveVersions(v1, "1", {b.performance}); }).find("only a newer version") != std::string::npos, "a move to the same or an older version is refused");
        // Not during a show.
        ::setenv("OSCAIRN_RELIABILITY", "instrument", 1);
        s.setPlaying(true);
        check(refusal([&] { s.moveVersions(v1, "2", {b.performance}); }) == "Stop the transport to move sounds", "at instrument, refused while the transport runs");
        s.setPlaying(false);
        ::unsetenv("OSCAIRN_RELIABILITY");
    }
    // Version 2 reading the gain at half: the change is listed, and the state written is version 2's.
    {
        home("b");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, changed, "2", true);
        const auto a = save(s, "Alpha");
        auto plan = wait(s, s.planVersionMove(v1, "2", {.items = {a.performance}}));
        const auto* i = item(plan, a.performance);
        check(i && i->status == "changed" && i->changes.size() >= 1 && i->changes[0].param == 7 && i->changes[0].name == "Gain" && std::abs(*i->changes[0].before - .8) < 1e-9 && std::abs(*i->changes[0].after - .4) < 1e-9,
              "the plan lists the parameter version 2 reads differently: " + status(plan, a.performance));
        const auto state = [&] { std::string s_; for(const auto& [path, bytes] : tree(pathOf(s, a.performance) / "nodes")) if(path.ends_with("state.clap")) s_ += bytes; return s_; };
        const auto old = state();
        auto moved = wait(s, s.moveVersions(v1, "2", {a.performance}));
        double gain = 0; const auto now = state();
        if(now.size() >= sizeof gain) std::memcpy(&gain, now.data(), sizeof gain);
        check(status(moved, a.performance) == "moved" && now != old && std::abs(gain - .4) < 1e-9, "the state is saved again as version 2 writes it: " + status(moved, a.performance) + ", gain " + std::to_string(gain) + ", " + std::to_string(old.size()) + " -> " + std::to_string(now.size()) + " bytes");
    }
    // Version 2 with other ports, without the claim: refused, naming the ports.
    {
        home("c");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, ports, "2", false);
        const auto a = save(s, "Alpha");
        auto moved = wait(s, s.moveVersions(v1, "2", {a.performance}, true));
        check(status(moved, a.performance).find("ports are not the same") != std::string::npos, "other ports refuse the item: " + status(moved, a.performance));
    }
    // Version 2 without the claim: one item at a time, and only when asked.
    {
        home("d");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", false);
        const auto a = save(s, "Alpha"), b = save(s, "Beta");
        auto plan = wait(s, s.planVersionMove(v1, "2", {.all = true}));
        check(!plan.claim && status(plan, a.performance) == "noClaim", "the plan says there is no claim: " + status(plan, a.performance));
        check(refusal([&] { s.moveVersions(v1, "2", {a.performance, b.performance}, true); }).find("does not say it opens") != std::string::npos, "more than one item is refused");
        check(refusal([&] { s.moveVersions(v1, "2", {a.performance}); }).find("does not say it opens") != std::string::npos, "one item needs withoutClaim");
        auto moved = wait(s, s.moveVersions(v1, "2", {a.performance}, true));
        check(status(moved, a.performance) == "moved", "one item moves with withoutClaim: " + status(moved, a.performance));
    }
    // The open Performance (step 3): one node moved makes a mix, all moved none; edit.undo puts version 1 back with its state.
    {
        home("e");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", true);
        const auto n1 = s.add("one", stored("1").string(), id), n2 = s.add("two", stored("1").string(), id);
        const auto p1 = s.addPart("One"), p2 = s.addPart("Two");
        s.insertIntoPart(p1, 0, n1); s.insertIntoPart(p2, 0, n2); s.commit();
        s.setParameter(n1, 7, .8); s.setParameter(n2, 7, .3); s.service();
        const auto version = [&](NodeId n) { for(const auto& x : s.graphState().topology.nodes) if(x.id == n) return x.plugin.version; return std::string(); };
        check(s.sessionStatus().mixedVersions.empty(), "one version: no mix");
        // All or none: the second node refusing its state as it is replaced (the 6th state load: two checks a node, then
        // the replacements) puts the first back too, and leaves no step of its own.
        {
            const auto nth = temp / "refuse-nth";
            std::ofstream(nth) << "6";
            ::setenv("KICK_FIXTURE_REFUSE_NTH", nth.c_str(), 1);
            const auto history = s.history().undo;
            const auto why = refusal([&] { s.useVersion(v1, "2"); });
            ::unsetenv("KICK_FIXTURE_REFUSE_NTH");
            s.service();
            check(!why.empty() && version(n1) == "1" && version(n2) == "1" && std::abs(s.parameter(n1, 7) - .8) < 1e-9 && std::abs(s.parameter(n2, 7) - .3) < 1e-9 && s.history().undo == history,
                  "a replacement failing puts back every node as it was, with no step: " + why + "; " + version(n1) + version(n2));
        }
        const auto used = s.useVersion(v1, "2", {n1});
        s.service();
        const auto mixed = s.sessionStatus().mixedVersions;
        check(version(n1) == "2" && version(n2) == "1" && used.changes.empty() && mixed.size() == 1 && mixed[0].id == id && mixed[0].versions == std::vector<std::string>{"1", "2"},
              "one node moved: two versions loaded, said in mixedVersions: " + version(n1) + " " + version(n2) + ", " + std::to_string(used.changes.size()) + " changes" + (used.changes.empty() ? std::string() : " (" + used.changes[0].name + " " + std::to_string(used.changes[0].param) + " " + (used.changes[0].before ? std::to_string(*used.changes[0].before) : "-") + " -> " + (used.changes[0].after ? std::to_string(*used.changes[0].after) : "-") + ")") + ", " + std::to_string(mixed.size()) + " mixed" +
                  (mixed.empty() ? std::string() : " " + mixed[0].id + " " + std::to_string(mixed[0].versions.size())));
        check(std::abs(s.parameter(n1, 7) - .8) < 1e-9 && s.sessionStatus().failures.empty(), "its setting kept");
        s.useVersion(v1, "2");
        s.service();
        check(version(n2) == "2" && s.sessionStatus().mixedVersions.empty() && std::abs(s.parameter(n2, 7) - .3) < 1e-9, "all moved: one version, settings kept");
        check(s.undo() && version(n2) == "1" && version(n1) == "2", "edit.undo puts version 1 back");
        s.service();
        check(std::abs(s.parameter(n2, 7) - .3) < 1e-9, "with its state");
        check(refusal([&] { s.useVersion(v1, "2", {n1}); }).find("does not play") != std::string::npos, "a node at another version is refused");
    }
    // The open Performance without the claim: only with withoutClaim; other ports refused before anything changes.
    {
        home("f");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, ports, "2", false);
        const auto n = s.add("one", stored("1").string(), id);
        const auto p = s.addPart("One"); s.insertIntoPart(p, 0, n); s.commit();
        const auto version = [&] { return s.graphState().topology.nodes.at(0).plugin.version; };
        check(refusal([&] { s.useVersion(v1, "2"); }).find("does not say it opens") != std::string::npos, "without the claim, refused");
        check(refusal([&] { s.useVersion(v1, "2", {}, true); }).find("ports are not the same") != std::string::npos && version() == "1", "other ports: refused, nothing changed");
    }
    // Removing unused versions (step 6): what keeps a version, the newest kept, nothing removed unless asked, and what
    // keeps a version is read again just before it goes.
    {
        home("g");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", true);
        auto listed = [&](const std::string& version) { for(const auto& p : s.installedPlugins()) if(p.id == id && p.version == version) return p; return Control::InstalledPlugin{}; };
        auto kinds = [&](const std::string& version) { std::string k; for(const auto& b : listed(version).keptBy) k += b.kind + " "; return k; };
        check(listed("2").newest && !listed("1").newest && listed("1").keptBy.empty(), "the newest version is marked; nothing keeps version 1");
        auto preview = s.removeUnusedPlugins();
        check(preview.size() == 1 && preview[0].version == "1" && preview[0].bytes > 0 && !preview[0].removed && fs::exists(stored("1")), "the preview offers version 1 alone, the newest never, and removes nothing");
        s.keepPlugin(v1, true);
        check(kinds("1") == "kept " && s.removeUnusedPlugins().empty(), "a version marked to keep is kept");
        s.keepPlugin(v1, false);
        const auto a = save(s, "Alpha");
        check(kinds("1") == "item item ", "items using it keep it: " + kinds("1"));
        {
            Session t(rate, frames);
            const auto n = t.add("synth", stored("1").string(), id);
            const auto p = t.addPart("Outside"); t.insertIntoPart(p, 0, n); t.commit();
            t.savePerformance((temp / "outside").string());
            const auto open = t.installedPlugins();
            check(std::any_of(open.begin(), open.end(), [&](const auto& p) { return p.version == "1" && std::count(p.keptBy.begin(), p.keptBy.end(), Control::KeptBy{"open", "the open Performance"}); }), "the open Performance keeps it");
        }
        check(kinds("1") == "item item outside ", "a Performance saved outside the library keeps it: " + kinds("1"));
        fs::remove_all(temp / "outside");
        auto moved = wait(s, s.moveVersions(v1, "2", {a.performance, a.sound}));
        check(kinds("1") == "move ", "a kept move keeps the version it left: " + kinds("1"));
        s.forgetVersionMove(moved.move);
        preview = s.removeUnusedPlugins();
        check(preview.size() == 1 && preview[0].version == "1", "unused again");
        // A source that cannot be read keeps every version, never counts as unused: an item's document, a saved
        // Performance's folder on a drive that is away, a kept move's journal.
        {
            const auto spare = save(s, "Spare");
            const auto doc = pathOf(s, spare.performance) / "performance.json";
            const auto bytes = slurp(doc);
            std::ofstream(doc) << "{ not json";
            check(kinds("1").find("unreadable") != std::string::npos && s.removeUnusedPlugins().empty(), "an unreadable item document keeps it: " + kinds("1"));
            std::ofstream(doc) << bytes;
            s.deleteLibraryItem(spare.performance); s.deleteLibraryItem(spare.sound);
            {
                Session t(rate, frames);
                const auto n = t.add("synth", stored("2").string(), id);
                const auto p = t.addPart("Away"); t.insertIntoPart(p, 0, n); t.commit();
                t.savePerformance((temp / "drive" / "away").string());
            }
            fs::rename(temp / "drive", temp / "drive-unplugged");
            check(kinds("1").find("unreadable") != std::string::npos, "a saved Performance on a drive that is away keeps it: " + kinds("1"));
            fs::rename(temp / "drive-unplugged", temp / "drive");
            fs::create_directories(fs::path(std::getenv("OSCAIRN_DATA_HOME")) / "Moves" / "2026-11-28-broken");
            std::ofstream(fs::path(std::getenv("OSCAIRN_DATA_HOME")) / "Moves" / "2026-11-28-broken" / "move.json") << "{ not json";
            check(kinds("1").find("unreadable") != std::string::npos, "an unreadable kept move keeps it: " + kinds("1"));
            fs::remove_all(fs::path(std::getenv("OSCAIRN_DATA_HOME")) / "Moves" / "2026-11-28-broken");
            // While a version job runs (a state load held 6 s), nothing is removed.
            const auto slow = temp / "slow-once";
            std::ofstream(slow) << "x";
            ::setenv("KICK_FIXTURE_SLOW_LOAD_ONCE", slow.c_str(), 1);
            const auto busy = save(s, "Busy");
            const auto job = s.planVersionMove(v1, "2", {.items = {busy.performance}});
            check(refusal([&] { s.removeUnusedPlugins({v1}); }).find("version job is running") != std::string::npos, "removal is refused while a version job runs");
            wait(s, job);
            ::unsetenv("KICK_FIXTURE_SLOW_LOAD_ONCE");
            s.deleteLibraryItem(busy.performance); s.deleteLibraryItem(busy.sound);
        }
        const auto b = save(s, "Beta"); // used between the preview and the removal
        auto done = s.removeUnusedPlugins({v1});
        check(done.size() == 1 && !done[0].removed && !done[0].keptBy.empty() && done[0].keptBy[0].kind == "item" && fs::exists(stored("1")), "a version used since the preview is kept, saying why");
        s.deleteLibraryItem(b.performance); s.deleteLibraryItem(b.sound);
        done = s.removeUnusedPlugins({v1, {"clap", id, "2"}});
        check(done.size() == 2 && done[0].removed && !fs::exists(stored("1")) && !done[1].removed && !done[1].keptBy.empty() && done[1].keptBy.back().kind == "newest" && fs::exists(stored("2")),
              "removed only when asked and unused; the newest stays: " + [&] { std::string o; for(const auto& d : done) { o += d.version + (d.removed ? " removed" : " kept"); for(const auto& k : d.keptBy) o += " " + k.kind + ":" + k.name; o += "; "; } return o; }());
    }
    std::printf("%s\n", failures ? "FAILED" : "PASSED");
    return failures ? 1 : 0;
}
