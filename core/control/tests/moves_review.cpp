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
    if(argc < 4) return 2;
    const char *fixture = argv[1], *v2 = argv[2];
    sign = shell(argv[3]);
    state_files::TemporaryDirectory scratch;
    temp = scratch.path;
    check(run(sign + " key " + shell(temp / "author.key")) == 0, "a key is made");
    check(run(sign + " key " + shell(temp / "other.key")) == 0, "another key is made");

    // A revoked key's claim is still honored: claimFor never reads the trust store.
    {
        home("a");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", true);
        const auto key = s.installedPlugins().at(1).claimKey;
        check(!key.empty(), "the claim is kept");
        Control::TrustedKey t; t.key = key; t.name = "author"; t.id = signing::keyId(key); t.revoked = true;
        s.setTrustedKey(t);
        auto plan = wait(s, s.planVersionMove(v1, "2", {.all = true}));
        check(!plan.claim, "a claim signed by a revoked key is not honored");
    }
    // Another key's archive with the same build bytes replaces the publisher's claim, though that key never signed the installed build.
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
    }
    // A move that cannot record itself (here: Moves is not a folder, as a full disk would fail it) must end, not leave the job running for good.
    {
        home("c");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", true);
        const auto a = save(s, "Alpha");
        { std::ofstream(fs::path(std::getenv("OSCAIRN_DATA_HOME")) / "Moves") << "x"; }
        const auto job = s.moveVersions(v1, "2", {a.performance}, false);
        std::string state = "running";
        for(int i = 0; i < 200 && state == "running"; ++i)
        {
            try { state = s.versionJob(job).state; } catch(const std::exception&) { state = "threw"; }
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
        check(state == "done", "the job ends: " + state);
        fs::remove(fs::path(std::getenv("OSCAIRN_DATA_HOME")) / "Moves");
        const auto again = refusal([&] { s.planVersionMove(v1, "2", {.all = true}); });
        check(again.empty(), "a later job can start: " + again);
    }
    return failures ? 1 : 0;
}
