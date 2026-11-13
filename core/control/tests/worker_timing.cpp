// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's timing report, for telling a worker's scheduling from its work: one block the plugin
// stalls in (the fixture's KICK_FIXTURE_STALL, which also first touches memory there) is silenced
// after the allowance; the blocks after it are not handed over while the worker is still busy;
// each of them is followed by a panic; then blocks are handed over again. engine.timing gives each
// part apart: the round trip, when the engine came for the block, the worker's wake-up and run (the
// stalled block's run too, recorded as the next block is handed over), the blocks not handed, the
// panics after a miss and the audio thread's page faults, counted from the last reset.
//   oscairn-worker-timing-test WORKER FIXTURE.clap
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
    if(argc != 3) return 2;
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
    std::printf("silent %llu, busy %llu, late %llu, panics after a miss %llu, faults %llu minor %llu major; round trip max %.0f us, collect max %.0f us, wake max %.0f us, run max %.0f us\n",
                (unsigned long long)w->silent, (unsigned long long)w->busy, (unsigned long long)w->late, (unsigned long long)w->missPanics, (unsigned long long)w->minorFaults,
                (unsigned long long)w->majorFaults, most(w->roundTrip) / 1e3, most(w->collect) / 1e3, most(w->wake) / 1e3, most(w->run) / 1e3);
    // One block waited for and silenced, then blocks not handed while the worker was busy (a slow
    // machine may miss another block or two besides).
    check(w->busy >= 10 && w->silent >= w->busy + 1 && w->silent <= w->busy + 3, "the stalled block is silenced after waiting, the rest of the stall's blocks are not handed: silent " +
          std::to_string(w->silent) + ", busy " + std::to_string(w->busy));
    check(w->missPanics == w->silent, "every block not rendered is followed by a panic: " + std::to_string(w->missPanics));
    check(most(w->roundTrip) >= 10'000'000 && most(w->roundTrip) < 100'000'000, "the longest round trip is the wait for the stalled block, no longer");
    check(most(w->collect) < most(w->roundTrip), "the engine came for the stalled block before it gave up on it");
    check(most(w->run) >= 100'000'000, "the stalled block's run, recorded as the next block was handed over: " + std::to_string(most(w->run)));
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
    for(const char* field : {"\"collect\":{", "\"wake\":{", "\"run\":{", "\"busy\":" , "\"missPanics\":", "\"minorFaults\":", "\"majorFaults\":"})
        check(reply.find(field) != std::string::npos, std::string("engine.timing gives each worker's ") + field);
    play(5);
    const auto after = session.engineTiming(false);
    check(after.workers.size() == 1 && after.workers[0].busy == 0 && after.workers[0].missPanics == 0 && after.workers[0].silent == 0, "counted from the reset");
#ifdef __linux__
    check(after.workers[0].minorFaults + after.workers[0].majorFaults < w->minorFaults + w->majorFaults, "page faults counted from the reset");
#endif
    return failures ? 1 : 0;
}
