// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's timing report, for telling a worker's scheduling from its work: one block the plugin
// stalls in (the fixture's KICK_FIXTURE_STALL, which also first touches memory there) is silenced
// after the allowance; the blocks after it are not handed over while the worker is still busy;
// none of them is followed by a panic (RFC 0045's miss policy); then blocks are handed over again. engine.timing gives each
// part apart: the round trip, when the engine came for the block, the worker's wake-up and run (the
// stalled block's run too, recorded as the next block is handed over), the blocks not handed, the
// panics after a miss and the audio thread's page faults, counted from the last reset. The audio thread's
// own CPU time per block (cpu) answers whether a long run was computing or not scheduled: far below the run
// for a stall that sleeps, close to it for one that spins (a second worker, KICK_FIXTURE_STALL's fourth field).
// An instrument two parts share (sound contexts, RFC 0021) is in it too, once, naming both parts.
//   oscairn-worker-timing-test WORKER FIXTURE.clap BASIC-SYNTH.clap
#include <oscairn/json_rpc.h>
#include <oscairn/session.h>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
}

int main(int argc, char** argv)
{
    if(argc != 4) return 2;
    // Times far from a busy machine's noise: a 10 ms allowance, a 100 ms stall (under the 250 ms hang
    // limit) in the plugin's 30th block, touching 4 MiB there.
    setenv("OSCAIRN_PLUGIN_WORKER", argv[1], 1);
    setenv("OSCAIRN_WORKER_BUDGET_US", "10000", 1);
    setenv("KICK_FIXTURE_STALL", "30:100000:4096", 1);
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    oscairn::Session session(rate, frames);
    const auto node = session.add("Stalling", argv[2], "org.kick.test.effect"); // an effect: handed over and taken apart (startProcess), as a plugin with sound contexts is
    session.connectAudio(node, oscairn::graphOutput);
    session.commit();
    float l[frames], r[frames]; float* out[]{l, r};
    auto play = [&](int blocks) { for(int i = 0; i < blocks; ++i) { session.process(out, frames); std::this_thread::sleep_for(std::chrono::milliseconds(1)); } };
    play(10);
    session.engineTiming(true);
    play(200); // the stall, its busy blocks while it lasts (about 100 ms), and blocks handed again after it
    const auto t = session.engineTiming(false);
    const oscairn::Control::Timing::Worker* w = nullptr;
    for(const auto& x : t.workers) if(x.node == node) w = &x;
    check(w != nullptr, "the worker is in engine.timing");
    if(!w) return 1;
    auto count = [](const oscairn::BlockTiming::Snapshot& s) { return s.steady.count() + s.startup.count(); };
    auto most = [](const oscairn::BlockTiming::Snapshot& s) { return std::max(s.steady.maxNs, s.startup.maxNs); };
    // The worst block's run, with the CPU time of that same block.
    auto worst = [](const oscairn::BlockTiming::Snapshot& s) -> const oscairn::Histogram& { return s.startup.maxNs > s.steady.maxNs ? s.startup : s.steady; };
    std::printf("silent %llu, busy %llu, late %llu, panics after a miss %llu, faults %llu minor %llu major; round trip max %.0f us, collect max %.0f us, wake max %.0f us, run max %.0f us\n",
                (unsigned long long)w->silent, (unsigned long long)w->busy, (unsigned long long)w->late, (unsigned long long)w->missPanics, (unsigned long long)w->minorFaults,
                (unsigned long long)w->majorFaults, most(w->roundTrip) / 1e3, most(w->collect) / 1e3, most(w->wake) / 1e3, most(w->run) / 1e3);
    // One block waited for and silenced, then blocks not handed while the worker was busy (a slow
    // machine may miss another block or two besides).
    check(w->busy >= 10 && w->silent >= w->busy + 1 && w->silent <= w->busy + 3, "the stalled block is silenced after waiting, the rest of the stall's blocks are not handed: silent " +
          std::to_string(w->silent) + ", busy " + std::to_string(w->busy));
    check(w->missPanics == 0, "no block not rendered is followed by a panic: " + std::to_string(w->missPanics));
    check(most(w->roundTrip) >= 10'000'000 && most(w->roundTrip) < 100'000'000, "the longest round trip is the wait for the stalled block, no longer");
    check(most(w->collect) < most(w->roundTrip), "the engine came for the stalled block before it gave up on it");
    check(most(w->run) >= 100'000'000, "the stalled block's run, recorded as the next block was handed over: " + std::to_string(most(w->run)));
    check(most(w->cpu) > 0, "the audio thread's CPU time is measured: " + std::to_string(most(w->cpu)));
    std::printf("worst block: run %.0f us, cpu %.0f us at %.2f s (sleeping stall); cpu max %.0f us; involuntary context switches %llu\n", worst(w->run).maxNs / 1e3, worst(w->run).maxCpuNs / 1e3,
                worst(w->run).maxAt / 1e9, most(w->cpu) / 1e3, (unsigned long long)w->involuntarySwitches);
    check(worst(w->run).maxNs >= 100'000'000 && worst(w->run).maxCpuNs < worst(w->run).maxNs / 5, "a stall that sleeps: the worst block's cpu is far below its run: run " +
          std::to_string(worst(w->run).maxNs) + ", cpu " + std::to_string(worst(w->run).maxCpuNs));
    check(worst(w->run).maxAt > 0, "the worst block's time in the window");
    // The window's last block may have been given up on, its wake and run not yet recorded.
    check(count(w->wake) == count(w->run) && count(w->roundTrip) - count(w->run) <= 1, "wake and run once for each block handed: " + std::to_string(count(w->run)) +
          " of " + std::to_string(count(w->roundTrip)));
#ifdef __linux__
    // At least one a 2 MiB page (transparent huge pages); one a 4 KiB page without them.
    check(w->minorFaults + w->majorFaults >= 2, "the audio thread's page faults, 4 MiB first touched: " + std::to_string(w->minorFaults + w->majorFaults));
#endif
    // Through the control protocol.
    oscairn::JsonRpc rpc(session);
    const auto reply = rpc.dispatch(R"({"jsonrpc":"2.0","id":1,"method":"engine.timing","params":{"reset":true}})").value_or("");
    for(const char* field : {"\"collect\":{", "\"wake\":{", "\"run\":{", "\"busy\":" , "\"missPanics\":", "\"minorFaults\":", "\"majorFaults\":", "\"cpu\":{", "\"involuntarySwitches\":", "\"maxCpu\":"})
        check(reply.find(field) != std::string::npos, std::string("engine.timing gives each worker's ") + field);
    play(5);
    const auto after = session.engineTiming(false);
    check(after.workers.size() == 1 && after.workers[0].busy == 0 && after.workers[0].missPanics == 0 && after.workers[0].silent == 0, "counted from the reset");
#ifdef __linux__
    check(after.workers[0].minorFaults + after.workers[0].majorFaults < w->minorFaults + w->majorFaults, "page faults counted from the reset");
#endif
    // A stall that computes: a second worker spinning 50 ms in its 30th block, so its cpu is close to its run.
    setenv("KICK_FIXTURE_STALL", "30:50000:0:1", 1);
    oscairn::Session spinning(rate, frames);
    const auto spinNode = spinning.add("Spinning", argv[2], "org.kick.test.effect");
    spinning.connectAudio(spinNode, oscairn::graphOutput);
    spinning.commit();
    for(int i = 0; i < 120; ++i) { spinning.process(out, frames); std::this_thread::sleep_for(std::chrono::milliseconds(1)); }
    const auto spun = spinning.engineTiming(false);
    check(spun.workers.size() == 1, "the second worker is in engine.timing");
    if(spun.workers.size() == 1)
    {
        const auto& h = worst(spun.workers[0].run);
        std::printf("worst block: run %.0f us, cpu %.0f us at %.2f s (spinning stall)\n", h.maxNs / 1e3, h.maxCpuNs / 1e3, h.maxAt / 1e9);
        check(h.maxNs >= 50'000'000 && h.maxCpuNs >= h.maxNs / 2 && h.maxCpuNs <= h.maxNs + 1'000'000, "a stall that computes: the worst block's cpu is close to its run: run " +
              std::to_string(h.maxNs) + ", cpu " + std::to_string(h.maxCpuNs));
    }
    // An instrument with sound contexts, two parts sharing its instance: one worker, both parts named.
    setenv("KICK_FIXTURE_STALL", "", 1);
    {
        oscairn::Session shared(rate, frames);
        const auto lead = shared.add("Lead", argv[3]);
        const auto alpha = shared.addPart("Alpha"); shared.insertIntoPart(alpha, 0, lead); shared.commit();
        const auto pad = shared.shareInstrument("Pad", lead);
        const auto beta = shared.addPart("Beta"); shared.insertIntoPart(beta, 0, pad); shared.commit();
        for(int i = 0; i < 40; ++i) { shared.service(); shared.process(out, frames); }
        const auto st = shared.engineTiming(false);
        check(st.workers.size() == 1, "a shared instrument is in engine.timing once: " + std::to_string(st.workers.size()));
        if(st.workers.size() == 1)
        {
            const auto& x = st.workers[0];
            check(x.node == lead && x.parts == std::vector<std::string>{"Alpha", "Beta"}, "under its first node, naming both parts it plays for");
            check(count(x.roundTrip) > 0 && count(x.wake) > 0 && count(x.cpu) > 0 && x.allowance > 0, "with its round trips and their parts");
        }
        const auto json = oscairn::JsonRpc(shared).dispatch(R"({"jsonrpc":"2.0","id":1,"method":"engine.timing"})").value_or("");
        check(json.find(R"("parts":["Alpha","Beta"])") != std::string::npos, "engine.timing gives the parts: " + json.substr(0, 200));
    }
    // RFC 0045's sharing bias: with OSCAIRN_SHARING=cpu a second Basic Synth added gets an instance, and a worker, of its own.
    for(const char* bias : {"", "cpu"})
    {
        setenv("OSCAIRN_SHARING", bias, 1);
        oscairn::Session two(rate, frames);
        const auto a = two.addPart("A"); two.insertIntoPart(a, 0, two.add("One", argv[3])); two.commit();
        const auto b = two.addPart("B"); two.insertIntoPart(b, 0, two.add("Two", argv[3])); two.commit();
        for(int i = 0; i < 10; ++i) { two.service(); two.process(out, frames); }
        const auto n = two.engineTiming(false).workers.size();
        check(n == (*bias ? 2u : 1u), std::string("OSCAIRN_SHARING=") + bias + ": " + std::to_string(n) + " workers for two Basic Synth parts");
    }
    unsetenv("OSCAIRN_SHARING");
    return failures ? 1 : 0;
}
