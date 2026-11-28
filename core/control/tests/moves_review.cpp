// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The independent review's cases for RFC 0043, each a finding the stack had to fix (from branch
// moves-review, 91b30e50, and the findings after it):
//  - D1: a claim whose key is revoked (or not trusted for plugins) is not honored;
//  - D2: another key's archive of the same version must not replace the publisher's recorded claim,
//    and plugins.list never shows a claim key that signed no build in the store;
//  - D3: a move that cannot record itself ends failed, the item untouched, and a later job can start;
//  - a crash at each step of a move and of an undo, then a start: every item is in the library, old
//    or new, opens, and the kept move still names each moved item;
//  - isolation (oscairn-moves-review-test ... isolation): a state that never loads and one that takes
//    too much memory each refuse their item, in a worker, and a cancel ends a waiting item at once.
//   oscairn-moves-review-test FIXTURE-V1 FIXTURE-V2 OSCAIRN-SIGN [isolation FIXTURE-V2-HANG FIXTURE-V2-HOG]
//   oscairn-moves-review-test --crash-move|--crash-undo HOME   (with OSCAIRN_MOVE_CRASH_AT set: the child)
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "../../plugin-host/src/StateFiles.h"
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <map>
#include <sstream>
#include <string>
#include <sys/wait.h>
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
    const Control::PluginVersion v1{"clap", id, "1"};

    std::string shell(const fs::path& p) { return "'" + p.string() + "'"; }
    int run(const std::string& command) { return std::system((command + " >/dev/null 2>&1").c_str()); }
    std::string slurp(const fs::path& f) { std::ifstream in(f, std::ios::binary); std::ostringstream out; out << in.rdbuf(); return out.str(); }
    fs::path home() { return std::getenv("OSCAIRN_DATA_HOME"); }
    void home(const fs::path& at) { fs::create_directories(at); ::setenv("OSCAIRN_DATA_HOME", at.c_str(), 1); }
    void home(const char* name) { home(temp / name); }
    // A build of the synth at `version`, signed, packed with or without the claim to open version 1's state, installed.
    void install(Session& s, const char* fixture, const std::string& version, bool claim, bool trust = false)
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
        if(trust) for(const auto& p : s.installedPlugins()) if(p.version == version && !p.claimKey.empty()) s.setTrustedKey({"", p.claimKey, "Author", {"plugins"}});
    }
    fs::path stored(const std::string& version) { return home() / "Plugins" / "clap" / (id + "@" + version) / platform() / "Synth.clap"; }
    // A part playing version 1 at gain .8, saved as a Performance.
    std::string save(Session& s, const std::string& name)
    {
        std::string out;
        {
            Session t(rate, frames);
            const auto n = t.add("synth", stored("1").string(), id);
            const auto p = t.addPart(name); t.insertIntoPart(p, 0, n); t.commit();
            t.setParameter(n, 7, .8); t.service();
            out = t.savePerformanceToLibrary(name, true);
        }
        s.libraryRescan();
        return out;
    }
    Control::VersionJob wait(Session& s, uint64_t job, int seconds = 60)
    {
        for(int i = 0; i < seconds * 50; ++i)
        {
            auto j = s.versionJob(job);
            if(j.state != "running") return j;
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
        check(false, "the job ends");
        return {};
    }
    std::string status(const Control::VersionJob& j, const std::string& itemId)
    {
        for(const auto& i : j.items) if(i.id == itemId) return i.status + (i.reason.empty() ? "" : " (" + i.reason + ")");
        return "absent";
    }
    fs::path pathOf(Session& s, const std::string& itemId) { Control::LibraryQuery q; q.id = itemId; const auto r = s.libraryQuery(q); return r.items.empty() ? fs::path() : fs::path(r.items[0].path); }
    std::string refusal(const std::function<void()>& call) { try { call(); } catch(const std::exception& e) { return e.what(); } return {}; }
    std::string versionOf(Session& s, const std::string& item)
    {
        const auto doc = slurp(pathOf(s, item) / "performance.json");
        return doc.find("\"version\":\"2\",\"pathHint\"") != std::string::npos ? "2" : doc.find("\"version\":\"1\",\"pathHint\"") != std::string::npos ? "1" : "?";
    }

    // The crash child: the move or the undo of the one kept move, in HOME, ended by OSCAIRN_MOVE_CRASH_AT.
    int crashChild(const std::string& what, const fs::path& at)
    {
        home(at);
        Session s(rate, frames);
        if(what == "--crash-undo") { s.undoVersionMove(s.versionMoves().at(0).move); return 0; }
        std::vector<std::string> items;
        for(const auto& i : s.libraryQuery({}).items) items.push_back(i.id);
        wait(s, s.moveVersions(v1, "2", items));
        return 0;
    }
    // After a crash: every item in the library, at one version or the other, opening without a stand-in; each at
    // version 2 named by the kept move, its old folder there; no copy or journal left half done.
    void recovered(const fs::path& at, const std::vector<std::string>& items, const std::string& what)
    {
        home(at);
        Session s(rate, frames);
        const auto moves = s.versionMoves();
        std::error_code none;
        for(const auto& e : fs::directory_iterator(at / "Moves", none))
            for(const auto& f : fs::directory_iterator(e.path(), none)) { const auto n = f.path().filename().string(); if(n.size() > 4 && n.substr(n.size() - 4) == ".new") check(false, what + ": a copy left beside the journal: " + n); }
        for(const auto& item : items)
        {
            const auto v = versionOf(s, item);
            check(v == "1" || v == "2", what + ": " + item + " is in the library at a version (" + v + ")");
            if(v == "2")
            {
                const bool named = !moves.empty() && std::count(moves[0].items.begin(), moves[0].items.end(), item) && fs::is_directory(at / "Moves" / moves[0].move / item);
                check(named, what + ": " + item + " moved and kept for undo");
            }
            s.loadPerformanceFromLibrary(item);
            check(s.sessionStatus().failures.empty(), what + ": " + item + " opens");
        }
        if(!moves.empty()) check(slurp(at / "Moves" / moves[0].move / "move.json").find("\"state\":\"done\"") != std::string::npos && moves[0].undoable, what + ": the kept move is settled");
    }
}

int main(int argc, char** argv)
{
    if(argc == 3 && (std::string(argv[1]) == "--crash-move" || std::string(argv[1]) == "--crash-undo")) return crashChild(argv[1], argv[2]);
    if(argc < 4) return 2;
    const char *fixture = argv[1], *v2 = argv[2];
    sign = shell(argv[3]);
    state_files::TemporaryDirectory scratch;
    temp = scratch.path;
    check(run(sign + " key " + shell(temp / "author.key")) == 0, "a key is made");
    check(run(sign + " key " + shell(temp / "other.key")) == 0, "another key is made");

    if(argc == 7 && std::string(argv[4]) == "isolation")
    {
        // A state the new build never loads, and one that takes 1 GiB (the test's worker limit is 256 MiB): each
        // refuses its item, the plugin in a worker of its own though no placement policy names one.
        for(const auto& [name, build] : std::map<std::string, const char*>{{"hang", argv[5]}, {"hog", argv[6]}})
        {
            home(name);
            Session s(rate, frames);
            install(s, fixture, "1", false);
            install(s, build, "2", true, true);
            const auto a = save(s, "Alpha");
            const auto started = std::chrono::steady_clock::now();
            auto plan = wait(s, s.planVersionMove(v1, "2", {.items = {a}}), 100);
            const auto took = std::chrono::duration<double>(std::chrono::steady_clock::now() - started).count();
            check(status(plan, a).rfind("refused", 0) == 0 && took < 90, name + ": refused in a worker within its limits: " + status(plan, a) + " after " + std::to_string(took) + " s");
            std::printf("%s: %s after %.1f s\n", name.c_str(), status(plan, a).c_str(), took);
            if(name == "hang")
            {
                // A cancel (the session going) ends an item waiting on its worker at once.
                const auto begun = std::chrono::steady_clock::now();
                {
                    Session t(rate, frames);
                    t.planVersionMove(v1, "2", {.items = {a}});
                    std::this_thread::sleep_for(std::chrono::seconds(3));
                }
                const auto gone = std::chrono::duration<double>(std::chrono::steady_clock::now() - begun).count();
                check(gone < 15, "the session goes at once though an item waits on a hung plugin: " + std::to_string(gone) + " s");
            }
        }
        return failures ? 1 : 0;
    }

    // D1: a claim whose key is revoked is not honored, nor one whose key the trust store does not trust for plugins.
    {
        home("a");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", true, true);
        const auto key = s.installedPlugins().at(1).claimKey;
        check(!key.empty(), "the claim is kept");
        check(wait(s, s.planVersionMove(v1, "2", {.all = true})).claim.has_value(), "a claim under a trusted key is honored");
        Control::TrustedKey t; t.key = key; t.name = "author"; t.id = signing::keyId(key); t.scope = {"plugins"}; t.revoked = true;
        s.setTrustedKey(t);
        auto plan = wait(s, s.planVersionMove(v1, "2", {.all = true}));
        check(!plan.claim, "a claim signed by a revoked key is not honored");
        t.revoked = false; t.scope = {"content"};
        s.setTrustedKey(t);
        check(!wait(s, s.planVersionMove(v1, "2", {.all = true})).claim, "nor one whose key is not trusted for plugins");
        const auto b = save(s, "Beta"), c = save(s, "Gamma");
        check(refusal([&] { s.moveVersions(v1, "2", {b, c}); }).find("does not say it opens") != std::string::npos, "without a trusted claim, two items are refused");
    }
    // D2: another key's archive with the same build bytes must not replace the publisher's claim, as that key never signed the installed build.
    {
        home("b");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", true);
        const auto folder = temp / "other-build";
        fs::create_directories(folder);
        fs::copy_file(v2, folder / "Synth.clap");
        check(run(sign + " build " + shell(temp / "other.key") + " clap " + id + " 2 " + platform() + " " + shell(folder / "Synth.clap")) == 0, "signed by the other key");
        check(run(sign + " plugin-package --license MIT --key " + shell(temp / "other.key") + " --opens-state-from " + id + " 0 " + shell(folder / "O.olib") + " clap " + id + " 2 Synth " + platform() + "=" + shell(folder / "Synth.clap")) == 0, "packed");
        s.installArchive((folder / "O.olib").string(), false, {}, true);
        const auto p = s.installedPlugins().at(1);
        check(p.opensStateFrom == "1", "another publisher's archive must not replace the recorded claim: now opensStateFrom=" + p.opensStateFrom);
        // A claim whose key signed no stored build (plugin.json edited, or its builds replaced since) is shown as none.
        const auto json = home() / "Plugins" / "clap" / (id + "@2") / "plugin.json";
        std::ofstream(json) << R"({"format":"clap","id":")" << id << R"(","version":"2","name":"Synth","license":"MIT","opensStateFrom":"1","claimKey":")" << std::string(64, 'a') << "\"}\n";
        const auto shown = s.installedPlugins().at(1);
        check(shown.opensStateFrom.empty() && shown.claimKey.empty(), "plugins.list shows no claim under a key that signed no stored build: " + shown.claimKey);
    }
    // D3: a move that cannot record itself (here: Moves is not a folder, as a full disk would fail it) ends failed, the
    // item as it was, and a later job can start.
    {
        home("c");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", true, true);
        const auto a = save(s, "Alpha");
        const auto before = slurp(pathOf(s, a) / "performance.json");
        { std::ofstream(home() / "Moves") << "x"; }
        const auto job = s.moveVersions(v1, "2", {a}, false);
        std::string state = "running";
        for(int i = 0; i < 500 && state == "running"; ++i)
        {
            try { state = s.versionJob(job).state; } catch(const std::exception&) { state = "threw"; }
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
        check(state == "failed" && !s.versionJob(job).error.empty(), "the job ends failed, saying why: " + state);
        check(slurp(pathOf(s, a) / "performance.json") == before && status(s.versionJob(job), a).rfind("refused", 0) == 0, "the item is as it was: " + status(s.versionJob(job), a));
        fs::remove(home() / "Moves");
        const auto again = refusal([&] { wait(s, s.planVersionMove(v1, "2", {.all = true})); });
        check(again.empty(), "a later job can start: " + again);
    }
    // A crash at each step of a move, then a start; and at each step of its undo.
    {
        const auto base = temp / "crash-base";
        std::vector<std::string> items;
        {
            home(base);
            Session s(rate, frames);
            install(s, fixture, "1", false);
            install(s, v2, "2", true, true);
            items = {save(s, "Alpha"), save(s, "Beta")};
        }
        for(int step = 1; step <= 7; ++step)
        {
            const auto at = temp / ("crash-move-" + std::to_string(step));
            fs::copy(base, at, fs::copy_options::recursive);
            const auto code = std::system(("OSCAIRN_MOVE_CRASH_AT=" + std::to_string(step) + " " + shell(fs::absolute(argv[0])) + " --crash-move " + shell(at) + " >/dev/null 2>&1").c_str());
            check(WIFEXITED(code) && WEXITSTATUS(code) == (step <= 6 ? 86 : 0), "the move's child ends at step " + std::to_string(step) + " (" + std::to_string(WEXITSTATUS(code)) + ")");
            recovered(at, items, "move, crash at step " + std::to_string(step));
        }
        const auto moved = temp / "crash-moved";
        fs::copy(base, moved, fs::copy_options::recursive);
        check(std::system((shell(fs::absolute(argv[0])) + " --crash-move " + shell(moved) + " >/dev/null 2>&1").c_str()) == 0, "a whole move");
        for(int step = 1; step <= 6; ++step)
        {
            const auto at = temp / ("crash-undo-" + std::to_string(step));
            fs::copy(moved, at, fs::copy_options::recursive);
            const auto code = std::system(("OSCAIRN_MOVE_CRASH_AT=" + std::to_string(step) + " " + shell(fs::absolute(argv[0])) + " --crash-undo " + shell(at) + " >/dev/null 2>&1").c_str());
            check(WIFEXITED(code) && WEXITSTATUS(code) == (step <= 5 ? 86 : 0), "the undo's child ends at step " + std::to_string(step) + " (" + std::to_string(WEXITSTATUS(code)) + ")");
            recovered(at, items, "undo, crash at step " + std::to_string(step));
        }
    }
    return failures ? 1 : 0;
}
