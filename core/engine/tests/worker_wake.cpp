// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0045, sleeping plugins: what waking a rested chain costs. An instrument (the fixture's sleep instrument) and N
// effects after it (the fixture's effect, sleeping on silent input: KICK_FIXTURE_EFFECT_SLEEP), each in its own
// worker, played in real time: CYCLES times, REST blocks with nothing for them, a note for SOUND blocks, let go.
// For chains of 1, 3 and 5 effects it prints the chain's time and each worker's wake (hand-over until its audio
// thread ran) and round trip on the block the note comes, against the same on a later sounding block, the idle
// block's cost (past the hold), and every block lost (late or silenced). With OSCAIRN_WORKER_REST unset (disabled) the
// workers can be played, so none rests; with OFF given, the instrument's note connection is off (draining, as a part a
// scene switches off) through each rest and back on in the very block of the note, so they rest and that enable wakes them.
// Fails on a lost block, or when a worker that may rest is called past the hold (or one that may not is rested).
// Absolute times do not transfer from this machine to the FANTOM; the comparison does.
//   oscairn-worker-wake-test WORKER FIXTURE [CYCLES [REST [FRAMES [EFFECTS [off]]]]]
#include <oscairn/graph.h>
#include <oscairn/plugin_host.h>
#include <oscairn/realtime.h>
#include <oscairn/timing.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
#include <thread>
#include <vector>

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: %s WORKER FIXTURE [CYCLES [REST [FRAMES [EFFECTS [off]]]]]\n", argv[0]); return 2; }
    const int cycles = argc > 3 ? std::atoi(argv[3]) : 50, rest = argc > 4 ? std::atoi(argv[4]) : 400;
    constexpr int sound = 8;
    setenv("KICK_FIXTURE_EFFECT_SLEEP", "1", 1);
    const char* restVar = std::getenv("OSCAIRN_WORKER_REST");
    const std::string mode = restVar ? restVar : "";
    const bool off = argc > 7 && std::string(argv[7]) == "off";
    const bool resting = mode == "all" || (mode != "0" && off); // whether an idle worker may rest here
    const uint32_t frames = argc > 5 ? uint32_t(std::atoi(argv[5])) : 128;
    constexpr int hold = 192; // WorkerHost's: blocks a plugin asleep is still called before it rests
    constexpr double rate = 48000;
    const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
    int failures = 0;
    oscairn::worker::makeRealtime(1e6 * frames / rate);
    for(const int effects : argc > 6 ? std::vector<int>{std::atoi(argv[6])} : std::vector<int>{1, 3, 5})
    for(int attempt = 0;; ++attempt)
    {
        std::vector<std::shared_ptr<oscairn::PluginHost>> hosts{oscairn::loadPluginInWorker(argv[1], argv[2], "org.kick.test.sleep", rate, frames)};
        for(int i = 0; i < effects; ++i) hosts.push_back(oscairn::loadPluginInWorker(argv[1], argv[2], "org.kick.test.effect", rate, frames));
        oscairn::Graph g(rate, frames);
        std::vector<oscairn::NodeId> nodes;
        for(auto& h : hosts) nodes.push_back(g.add(h));
        g.connectNotes(oscairn::graphInput, nodes[0]);
        for(size_t i = 1; i < nodes.size(); ++i) g.connectAudio(nodes[i - 1], nodes[i]);
        g.connectAudio(nodes.back(), oscairn::graphOutput);
        g.commit();
        std::vector<float> l(frames), r(frames); float* out[]{l.data(), r.data()};
        auto next = std::chrono::steady_clock::now();
        auto block = [&](std::vector<kick::clap::Event> e = {}) {
            next += period;
            std::this_thread::sleep_until(next);
            const auto t = std::chrono::steady_clock::now();
            g.process(out, frames, nullptr, e.data(), uint32_t(e.size()));
            return std::chrono::duration<double, std::micro>(std::chrono::steady_clock::now() - t).count();
        };
        auto lost = [&] { uint64_t n = 0; for(auto& h : hosts) { const auto s = h->statistics(); n += s.lateBlocks + s.silentBlocks; } return n; };
        auto called = [&] { uint64_t n = 0; for(auto& h : hosts) n += h->statistics().calledBlocks; return n; };
        // The one block measured: each worker's wake and round trip as the engine took it (a window holding that block alone).
        struct Took { double chain = 0; std::vector<double> wake, trip; };
        auto measured = [&](std::vector<kick::clap::Event> e) {
            for(auto& h : hosts) { h->workerTiming()->reset(); }
            Took t; t.chain = block(std::move(e));
            for(auto& h : hosts)
            {
                auto max = [](const oscairn::BlockTiming::Snapshot& s) { return std::max(s.steady.maxNs, s.startup.maxNs) / 1e3; };
                t.wake.push_back(max(h->workerTiming()->wake.read())); t.trip.push_back(max(h->workerTiming()->roundTrip.read()));
            }
            return t;
        };
        for(int i = 0; i < 600; ++i) block(); // past the workers' start (BlockTiming's warm-up)
        const auto lostBefore = lost();
        double idle = 0, wakeChain = 0, steadyChain = 0, wakeMax = 0, steadyMax = 0;
        std::vector<double> wake(hosts.size()), trip(hosts.size()), steadyWake(hosts.size()), steadyTrip(hosts.size());
        uint64_t idleBlocks = 0, idleCalls = 0;
        for(int c = 0; c < cycles; ++c)
        {
            const auto calls = called();
            if(off) { g.setDraining(oscairn::graphInput, nodes[0], true); g.commit(); }
            for(int i = 0; i < rest; ++i) { const auto t = block(); if(i >= hold + 2) { idle += t; ++idleBlocks; } }
            idleCalls += called() - calls;
            if(off) { g.setDraining(oscairn::graphInput, nodes[0], false); g.commit(); } // on again, taken in the note's own block
            const auto w = measured({kick::clap::Event::note(true, 60, 1)});
            Took s;
            for(int i = 1; i < sound; ++i) { const auto t = i == sound / 2 ? measured({}) : Took{block()}; if(i == sound / 2) s = t; }
            block({kick::clap::Event::note(false, 60, 0)});
            wakeChain += w.chain; steadyChain += s.chain; wakeMax = std::max(wakeMax, w.chain); steadyMax = std::max(steadyMax, s.chain);
            for(size_t k = 0; k < hosts.size(); ++k) { wake[k] += w.wake[k]; trip[k] += w.trip[k]; steadyWake[k] += s.wake[k]; steadyTrip[k] += s.trip[k]; }
        }
        const auto lostBlocks = lost() - lostBefore;
        std::printf("1 instrument + %d effects, %d cycles of %d rested blocks (rest %s%s): chain %.1f us on the waking block (max %.1f), %.1f sounding (max %.1f); "
                    "idle block %.2f us, %llu worker calls in %llu idle blocks; lost %llu\n",
                    effects, cycles, rest, mode.empty() ? "disabled" : mode.c_str(), off ? ", part off while resting" : "", wakeChain / cycles, wakeMax, steadyChain / cycles, steadyMax, idle / std::max<uint64_t>(1, idleBlocks),
                    (unsigned long long)idleCalls, (unsigned long long)idleBlocks, (unsigned long long)lostBlocks);
        for(size_t k = 0; k < hosts.size(); ++k)
            std::printf("  worker %zu: wake %.1f us waking, %.1f sounding; round trip %.1f waking, %.1f sounding\n",
                        k, wake[k] / cycles, steadyWake[k] / cycles, trip[k] / cycles, steadyTrip[k] / cycles);
        // A wake that cost a block would lose one every cycle, in every attempt; another process on a shared runner holding the engine up
        // loses a stray block now and then, so a configuration is measured up to three times and stands when one run loses none.
        if(lostBlocks && attempt < 2) { std::printf("  %llu blocks lost: measured again\n", (unsigned long long)lostBlocks); continue; }
        if(lostBlocks) { std::fprintf(stderr, "FAIL: %llu blocks lost\n", (unsigned long long)lostBlocks); ++failures; }
        // An idle worker that may rest called only for the hold; one that may not, every block.
        const uint64_t all = uint64_t(rest) * cycles * hosts.size();
        if(resting ? idleCalls > uint64_t(hold + 2) * cycles * hosts.size() || (rest > hold + 2 && idleCalls >= all) : idleCalls < all) { std::fprintf(stderr, "FAIL: %llu worker calls in idle blocks of %llu\n", (unsigned long long)idleCalls, (unsigned long long)all); ++failures; }
        break;
    }
    return failures ? 1 : 0;
}
