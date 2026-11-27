// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A plugin that refuses its saved state (its state load returns false) is never left playing its
// default sound unseen (oscairn#18, part 1). The fixture refuses every load while a file exists.
//  - A Performance whose plugin refuses its state is refused, naming the plugin and why, and the
//    Performance that was open stays as it was.
//  - A Sound loaded into a part likewise throws, and the part keeps what it had.
//  - A plugin that has no saved state is not refused anything.
//  - A worker killed while its plugin refuses the state it restarts with stays failed (a failure the
//    session status lists, with the reason), and plays again once the plugin takes the state.
//   oscairn-refused-state-test FIXTURE.clap [WORKER]
#include "oscairn/session.h"
#include "../../plugin-host/src/StateFiles.h"
#include <chrono>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>
#include <thread>
#include <unistd.h>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128;

    std::string refusal(const std::function<void()>& call)
    {
        try { call(); } catch(const std::exception& e) { return e.what(); }
        return {};
    }
    bool has(Session& s, const char* name) { try { return s.hasNode(s.nodeId(name)); } catch(const std::exception&) { return false; } }
    std::string failureOf(Session& s) { std::string all; for(const auto& f : s.sessionStatus().failures) all += f.name + ": " + f.reason + "; "; return all; }
    // Kills the engine's own worker processes, no one else's.
    void killChildren()
    {
        FILE* p = popen(("pgrep -P " + std::to_string(::getpid())).c_str(), "r");
        int pid = 0; while(p && std::fscanf(p, "%d", &pid) == 1) ::kill(pid, SIGKILL);
        if(p) pclose(p);
    }
}

int main(int argc, char** argv)
{
    if(argc < 2) return 2;
    const char* fixture = argv[1];
    const bool workers = argc > 2;
    if(workers) ::setenv("OSCAIRN_PLUGIN_WORKER", argv[2], 1);
    state_files::TemporaryDirectory temp;
    fs::create_directories(temp.path / "home");
    ::setenv("OSCAIRN_DATA_HOME", (temp.path / "home").c_str(), 1);
    const auto refuse = temp.path / "refuse";
    ::setenv("KICK_FIXTURE_REFUSE_STATE", refuse.c_str(), 1);
    const auto perf = temp.path / "perf", plain = temp.path / "plain";
    std::string sound;
    {
        Session s(rate, frames);
        const auto n = s.add("synth", fixture, "org.kick.test.synth");
        const auto p = s.addPart("Lead"); s.insertIntoPart(p, 0, n); s.commit();
        s.setParameter(n, 7, .8); s.service();
        s.savePerformance(perf.string());
        sound = s.savePartAsSound(p, "Lead").id;
    }
    {
        Session s(rate, frames);
        const auto n = s.add("bare", fixture, "org.kick.test.no-state");
        const auto p = s.addPart("Bare"); s.insertIntoPart(p, 0, n); s.commit();
        s.savePerformance(plain.string());
    }
    std::ofstream(refuse) << "x";
    // Opening.
    {
        Session s(rate, frames);
        const auto why = refusal([&] { s.loadPerformance(perf.string()); });
        check(why.find("synth") != std::string::npos && why.find("org.kick.test.synth") != std::string::npos && why.find("Plugin rejected state") != std::string::npos,
              "a Performance whose plugin refuses its state is refused, naming it and why: " + why);
        check(s.sessionStatus().failures.empty() && s.sessionStatus().notices.empty(), "and leaves nothing behind");
        check(refusal([&] { s.loadPerformance(plain.string()); }).empty(), "a plugin with no saved state is not refused anything");
    }
    // A Sound into a part.
    {
        Session s(rate, frames);
        const auto n = s.add("keep", fixture, "org.kick.test.effect");
        const auto p = s.addPart("Other"); s.insertIntoPart(p, 0, n); s.commit();
        const auto why = refusal([&] { s.loadSound(p, sound); });
        check(why.find("Plugin rejected state") != std::string::npos, "a Sound whose plugin refuses its state is not loaded: " + why);
        check(has(s, "keep") && !has(s, "synth"), "and the part keeps its plugin");
        std::filesystem::remove(refuse);
        check(refusal([&] { s.loadSound(p, sound); }).empty() && has(s, "synth"), "the same Sound loads once the plugin takes its state");
        std::ofstream(refuse) << "x";
    }
    // A Performance change: the instance the open one frees is offered to the next, whose state it refuses,
    // and a fresh one refuses too: the change is refused and the open Performance plays on.
    {
        const auto second = temp.path / "second";
        std::filesystem::remove(refuse);
        Session s(rate, frames);
        s.loadPerformance(perf.string());
        s.setParameter(s.nodeId("synth"), 7, .3); s.service();
        s.savePerformance(second.string());
        s.loadPerformance(plain.string()); // the synth is free now, for the next to take over
        std::ofstream(refuse) << "x";
        const auto why = refusal([&] { s.loadPerformance(second.string()); });
        check(why.find("Plugin rejected state") != std::string::npos, "a change to a Performance whose plugin refuses its state is refused: " + why);
        check(has(s, "bare") && s.sessionStatus().failures.empty(), "and the open one plays on");
    }
    // A worker restarted with a state its plugin refuses.
    if(workers)
    {
        std::filesystem::remove(refuse);
        Session s(rate, frames);
        s.loadPerformance(perf.string());
        check(failureOf(s).empty(), "opened with no refusal");
        // Two refusals, then the plugin takes the state: a worker is restarted at most three times in a minute, and the
        // supervisor retries at once, so a test removing the file on seeing the first refusal races the third try.
        std::ofstream(refuse) << "2";
        killChildren();
        std::string why;
        float l[frames]{}, r[frames]{}; float* out[]{l, r};
        for(int i = 0; i < 10000 && why.find("Plugin rejected state") == std::string::npos; ++i) { s.service(); s.process(out, frames); why = failureOf(s); std::this_thread::sleep_for(std::chrono::milliseconds(1)); }
        check(why.find("synth") != std::string::npos && why.find("Plugin rejected state") != std::string::npos, "a restarted worker whose plugin refuses its state stays failed, saying why: " + why);
        for(int i = 0; i < 1500 && !failureOf(s).empty(); ++i) { s.service(); s.process(out, frames); std::this_thread::sleep_for(std::chrono::milliseconds(10)); }
        check(failureOf(s).empty() && !std::filesystem::exists(refuse), "and plays again once it takes the state: " + failureOf(s));
    }
    return failures ? 1 : 0;
}
