// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0045, sleeping plugins: a plugin in a worker that says it is asleep (CLAP_PROCESS_SLEEP) is not
// called while nothing comes for it, and is called on the very block something does: a note, a host
// command, a panic; with native sound contexts (oscairn-clap's sound-snapshots example), a select and a
// choke_retiring. engine.timing's counts (calledBlocks, restedBlocks) say which blocks were which, and its
// steady time goes on through the blocks it slept. A plugin taking the beat and harmony (the follower fixture, asleep after
// every block as an idle arpeggiator) rests too: the beat alone does not wake it, the transport starting does, and it gets
// the free-running beat's position then. With OSCAIRN_WORKER_REST=0 every worker is called every block instead, and its
// host still leaves a plugin asleep uncalled for the beat alone. Those run with OSCAIRN_WORKER_REST=all; unset (disabled),
// a worker any note can reach is called every block too, and one whose every note connection is off (draining) rests:
// a note in the very block its connection comes back on is played, the worker called on that block.
//   oscairn-worker-sleep-test WORKER FIXTURE SOUND-SNAPSHOTS-FIXTURE FOLLOWER-FIXTURE
#include <oscairn/graph.h>
#include <oscairn/plugin_host.h>
#include <oscairn/transport.h>
#include <clap/clap.h>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

namespace
{
int failures = 0;
void check(bool ok, const std::string& what)
{
    if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; }
}
}

int main(int argc, char** argv)
{
    if(argc < 5) { std::fprintf(stderr, "usage: %s WORKER FIXTURE SOUND-SNAPSHOTS-FIXTURE FOLLOWER-FIXTURE\n", argv[0]); return 2; }
    const char* restVar = std::getenv("OSCAIRN_WORKER_REST");
    const std::string mode = restVar ? restVar : "";
    const bool resting = mode == "all", disabled = mode != "all" && mode != "0";
    // A block it slept through: rested, or with resting off (or its node playable) called (its host leaving the plugin uncalled).
    auto slept = [&](std::pair<uint64_t, uint64_t> b) { return resting ? b == std::pair<uint64_t, uint64_t>{0, 1} : b == std::pair<uint64_t, uint64_t>{1, 0}; };
    // WorkerHost's hold: a plugin asleep is still called for this many blocks in a row before it rests; `after` the
    // block that follows them.
    constexpr int hold = 192;
    auto after = [&](auto& block) { for(int i = 0; i < hold; ++i) block(); return block(); };
    constexpr uint32_t frames = 64;
    constexpr double rate = 48000;
    try
    {
        auto host = oscairn::loadPluginInWorker(argv[1], argv[2], "org.kick.test.sleep", rate, frames, oscairn::ProcessingMode::Offline);
        oscairn::Graph g(rate, frames);
        const auto n = g.add(host);
        g.connectNotes(oscairn::graphInput, n); g.connectAudio(n, oscairn::graphOutput); g.commit();
        std::vector<float> l(frames), r(frames); float* out[]{l.data(), r.data()};
        auto block = [&](std::vector<kick::clap::Event> e = {}) {
            const auto before = host->statistics();
            g.process(out, frames, nullptr, e.data(), uint32_t(e.size()));
            const auto after = host->statistics();
            return std::pair{after.calledBlocks - before.calledBlocks, after.restedBlocks - before.restedBlocks};
        };
        check(block().first == 1, "its first block is called (it has not slept yet)");
        check(host->asleep(), "silent, it says it is asleep");
        uint32_t rested = 0;
        check(!resting || block().first == 1, "asleep a moment ago, it is still called (the hold)");
        for(int i = 0; i < hold; ++i) block();
        for(int i = 0; i < 20; ++i) rested += slept(block());
        check(rested == 20, "asleep with nothing for it: " + std::to_string(rested) + " of 20 blocks slept through");
        check(l[0] == 0 && r[frames - 1] == 0, "its outputs are silence");
        check(block({kick::clap::Event::note(true, 60, 1)}) == std::pair<uint64_t, uint64_t>{1, 0}, "a note wakes it on that very block");
        check(!host->asleep() && l[0] != 0, "and it sounds");
        block({kick::clap::Event::note(false, 60, 0)});
        for(int i = 0; i < 3; ++i) block();
        check(host->asleep() && slept(after(block)), "let go, it sleeps again");
        // A host command (a parameter set) reaching the plugin wakes it for the next block, as in the engine's process.
        clap_event_param_value_t v{}; v.header = {sizeof v, 0, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE, 0};
        v.param_id = 7; v.note_id = -1; v.port_index = v.channel = v.key = -1; v.value = .3;
        check(host->sendEvent(kick::clap::Event::from(v)), "the command is taken");
        check(block().first == 1, "a host command wakes it on the next block");
        check(slept(after(block)), "then it sleeps again");
        host->panic();
        check(block().first == 1, "a panic wakes it on the next block");
        check(slept(after(block)), "and after it, asleep again");
        // Its steady time went on through the blocks it slept: the worker took them as its own idle blocks, told with
        // the next block it was called for.
        block({kick::clap::Event::note(true, 62, 1)});
        const auto s = host->statistics();
        check(s.idleBlocks >= s.restedBlocks && s.idleBlocks >= 20, "the blocks slept through count as the plugin's idle blocks: " + std::to_string(s.idleBlocks) + " of " + std::to_string(s.restedBlocks));
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    // A select (an accepted one) and a queued choke_retiring reaching a plugin asleep wake it for the next block.
    try
    {
        auto host = oscairn::loadPluginInWorker(argv[1], argv[3], "org.oscairn.example.sound-snapshots", rate, frames, oscairn::ProcessingMode::Offline);
        check(host->soundContextCapacity() > 0 && host->ringsOut(), "the example has sound contexts and ring-out");
        const auto a = host->prepareSound("A", {}), b = host->prepareSound("B", {});
        check(host->selectSound(a), "A selected");
        oscairn::Graph g(rate, frames);
        const auto n = g.add(host);
        g.connectNotes(oscairn::graphInput, n); g.connectAudio(n, oscairn::graphOutput); g.commit();
        std::vector<float> l(frames), r(frames); float* out[]{l.data(), r.data()};
        auto block = [&] {
            const auto before = host->statistics();
            g.process(out, frames, nullptr, nullptr, 0);
            const auto after = host->statistics();
            return std::pair{after.calledBlocks - before.calledBlocks, after.restedBlocks - before.restedBlocks};
        };
        for(int i = 0; i < 3; ++i) block();
        check(host->asleep() && slept(after(block)), "idle, the example sleeps and is not called");
        check(host->selectSound(b), "B selected");
        check(block().first == 1, "a select wakes it on the next block");
        check(slept(after(block)), "then it sleeps again");
        check(host->chokeRetiring(), "choke_retiring queued");
        check(block().first == 1, "a choke_retiring wakes it on the next block");
        check(slept(after(block)), "and it sleeps again");
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    // The beat and harmony wake no plugin asleep: the follower, asleep after every block, is not called for the beat alone
    // (its outputs silence), and the transport starting wakes it with the free-running beat's position at that block.
    try
    {
        setenv("FOLLOWER_SLEEPS", "1", 1); // the worker inherits it
        auto host = oscairn::loadPluginInWorker(argv[1], argv[4], "org.oscairn.test.follower", rate, frames, oscairn::ProcessingMode::Offline);
        oscairn::Graph g(rate, frames);
        const auto n = g.add(host);
        g.connectAudio(n, oscairn::graphOutput); g.commit();
        oscairn::Transport transport(rate, 120);
        transport.setPlaying(false);
        std::vector<float> l(frames), r(frames); float* out[]{l.data(), r.data()};
        int64_t at = 0;
        auto block = [&] {
            const auto before = host->statistics();
            const auto t = transport.current();
            g.process(out, frames, &t, nullptr, 0);
            transport.advance(frames); at += frames;
            const auto after = host->statistics();
            return std::pair{after.calledBlocks - before.calledBlocks, after.restedBlocks - before.restedBlocks};
        };
        check(block().first == 1 && host->asleep(), "the follower is called first and says it is asleep");
        uint32_t rested = 0; bool silent = true;
        for(int i = 0; i < hold; ++i) block();
        for(int i = 0; i < 20; ++i) { rested += slept(block()); silent = silent && g.audioOutput(n, 2)[0] == 0; }
        check(rested == 20 && silent, "a beat every block does not wake it: " + std::to_string(rested) + " of 20 blocks slept through");
        transport.setPlaying(true);
        const auto beatAt = double(at) * 2 / rate; // 120 BPM from the graph's start
        check(block().first == 1, "the transport starting wakes it");
        check(std::abs(g.audioOutput(n, 2)[0] - beatAt) < 1e-3, "with the free-running beat's position: " + std::to_string(g.audioOutput(n, 2)[0]) + " against " + std::to_string(beatAt));
        unsetenv("FOLLOWER_SLEEPS");
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    // The policy's own case (disabled): the instrument's note connection off (draining, as a part a scene switches off),
    // so no note can reach it; it rests. Turned back on, with a note in that very block: called on it, and it sounds.
    if(disabled) try
    {
        auto host = oscairn::loadPluginInWorker(argv[1], argv[2], "org.kick.test.sleep", rate, frames, oscairn::ProcessingMode::Offline);
        oscairn::Graph g(rate, frames);
        const auto n = g.add(host);
        g.connectNotes(oscairn::graphInput, n); g.connectAudio(n, oscairn::graphOutput); g.commit();
        std::vector<float> l(frames), r(frames); float* out[]{l.data(), r.data()};
        auto block = [&](std::vector<kick::clap::Event> e = {}) {
            const auto before = host->statistics();
            g.process(out, frames, nullptr, e.data(), uint32_t(e.size()));
            const auto after = host->statistics();
            return std::pair{after.calledBlocks - before.calledBlocks, after.restedBlocks - before.restedBlocks};
        };
        uint64_t rested = 0;
        for(int i = 0; i < hold + 20; ++i) rested += block().second;
        check(rested == 0, "playable, it is never rested: " + std::to_string(rested));
        g.setDraining(oscairn::graphInput, n, true); g.commit();
        rested = 0;
        for(int i = 0; i < hold + 20; ++i) rested += block().second;
        check(rested >= 19, "its note connection off, it rests after the hold: " + std::to_string(rested));
        g.setDraining(oscairn::graphInput, n, false); g.commit();
        check(block({kick::clap::Event::note(true, 60, 1)}) == std::pair<uint64_t, uint64_t>{1, 0} && l[0] != 0, "back on with a note in the same block: called on it, and it sounds");
        check(host->statistics().lateBlocks + host->statistics().silentBlocks == 0, "no block lost");
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(!failures) std::printf("worker sleep (rest %s): asleep, not called; called on the block a note, a command, a panic, a select, a choke_retiring or a transport change comes, not for the beat\n", mode.empty() ? "disabled" : mode.c_str());
    return failures ? 1 : 0;
}
