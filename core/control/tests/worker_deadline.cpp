// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045: no wait on a worker extends the audio callback past its deadline. A worker whose
// plugin stalls 230 ms in one block, with an allowance of 200 ms: with the callback's deadline set
// (as the device's callback sets it), the engine gives the block up at the deadline instead of
// waiting the allowance; with the extra block it collects the last block only if it is done, never
// waiting. Without a deadline (a host with no device) the allowance stands. With step 6
// (OSCAIRN_WORKER_DEADLINE), a chain of two workers whose blocks take far longer than their
// allowance plays within a generous deadline, where the allowance alone silences them.
// The extra block in real time (blocks a period apart): a worker whose every block takes 2.4 ms
// of a 2.667 ms period misses none; one taking 5.5 ms misses every block, the callback never
// running past its deadline; among 1 ms blocks one of 4 ms costs none, one of 5 ms at most one, one of
// 5.5 ms (past two periods, its worker busy through the next hand-over) at most two.
//   oscairn-worker-deadline-test WORKER FIXTURE.clap
#include <oscairn/session.h>
#include <oscairn/realtime.h>
#include <oscairn/plugin_host.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    using clock = std::chrono::steady_clock;

    struct Run { double longest = 0; uint64_t silent = 0, late = 0; };
    // `blocks` blocks a millisecond apart, each with the callback's deadline `deadline` after its start (none: unset).
    Run play(const char* fixture, bool chain, int blocks, std::chrono::microseconds deadline)
    {
        oscairn::Session session(rate, frames);
        const auto part = session.addPart("Part");
        session.insertIntoPart(part, 0, session.add("First", fixture, "org.kick.test.effect"));
        if(chain) session.insertIntoPart(part, 1, session.add("Second", fixture, "org.kick.test.effect"));
        session.commit();
        float l[frames], r[frames]; float* out[]{l, r};
        Run run;
        for(int i = 0; i < blocks; ++i)
        {
            const auto start = clock::now();
            if(deadline.count()) oscairn::worker::callbackDeadline = start + deadline;
            session.process(out, frames);
            oscairn::worker::callbackDeadline = {};
            if(i >= 10) run.longest = std::max(run.longest, std::chrono::duration<double, std::milli>(clock::now() - start).count());
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }
        for(const auto& w : session.engineTiming(false).workers) { run.silent += w.silent; run.late += w.late; }
        return run;
    }

    // The extra block in real time: `blocks` blocks a period apart, each callback with its deadline, the
    // fixture's work per block KICK_FIXTURE_WORK_US. Its blocks lost, callbacks past the period, the longest.
    struct Paced { uint64_t silent = 0, busy = 0, panics = 0; int over = 0; double longest = 0; };
    Paced paced(const char* fixture, int blocks)
    {
        setenv("OSCAIRN_WORKER_EXTRA_BLOCK", "org.kick.test.effect", 1);
        oscairn::Session session(rate, frames);
        const auto part = session.addPart("Part");
        session.insertIntoPart(part, 0, session.add("Slow", fixture, "org.kick.test.effect"));
        session.commit();
        unsetenv("OSCAIRN_WORKER_EXTRA_BLOCK");
        float l[frames], r[frames]; float* out[]{l, r};
        const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
        Paced p;
        auto next = clock::now();
        for(int i = -50; i < blocks; ++i) // the first 50 to start with, not counted
        {
            if(!i) { session.engineTiming(true); p = {}; }
            std::this_thread::sleep_until(next); next += period;
            const auto start = clock::now();
            oscairn::worker::callbackDeadline = start + std::chrono::nanoseconds(int64_t(double(period.count()) * (1 - oscairn::worker::mixingReserve)));
            session.process(out, frames);
            oscairn::worker::callbackDeadline = {};
            const auto took = clock::now() - start;
            p.longest = std::max(p.longest, std::chrono::duration<double, std::milli>(took).count());
            if(took > period) ++p.over;
        }
        for(const auto& w : session.engineTiming(false).workers) { p.silent += w.silent; p.busy += w.busy; p.panics += w.missPanics; }
        return p;
    }
    // The best of three runs: a busy machine only adds losses and long callbacks.
    template<class Better> Paced bestOf(const char* fixture, int blocks, Better better)
    {
        Paced b = paced(fixture, blocks);
        for(int i = 1; i < 3; ++i) if(const auto r = paced(fixture, blocks); better(r, b)) b = r;
        return b;
    }
}

int main(int argc, char** argv)
{
    if(argc != 3) return 2;
    setenv("OSCAIRN_PLUGIN_WORKER", argv[1], 1);
    setenv("OSCAIRN_WORKER_BUDGET_US", "200000", 1);
    const auto deadline = std::chrono::microseconds(int64_t(2667 * (1 - oscairn::worker::mixingReserve)));
    // The plugin's 30th block sleeps 230 ms (under the 250 ms hang limit).
    setenv("KICK_FIXTURE_STALL", "30:230000", 1);
    const auto none = play(argv[2], false, 80, {});
    std::printf("no deadline: longest block %.1f ms, silent %llu\n", none.longest, (unsigned long long)none.silent);
    check(none.longest >= 190, "without a deadline the engine waits the allowance for the stalled block: " + std::to_string(none.longest) + " ms");
    // The bound: the deadline plus a slack far below the allowance. A machine's stalls only lengthen a block, and a
    // wait to the allowance makes every run take 200 ms or more, so the best of three runs within it shows the rule
    // even where a busy host stalls one run (a container on one has shown 152 ms).
    auto best = [&](bool extraBlock) {
        if(extraBlock) setenv("OSCAIRN_WORKER_EXTRA_BLOCK", "org.kick.test.effect", 1);
        Run b; b.longest = 1e9;
        for(int i = 0; i < 3 && b.longest >= 2.3 + 90; ++i) if(const auto r = play(argv[2], false, 80, deadline); r.longest < b.longest) b = r;
        unsetenv("OSCAIRN_WORKER_EXTRA_BLOCK");
        return b;
    };
    const auto due = best(false);
    std::printf("deadline: longest block %.1f ms, silent %llu\n", due.longest, (unsigned long long)due.silent);
    check(due.longest < 2.3 + 90, "with the callback's deadline no block waits past it: " + std::to_string(due.longest) + " ms");
    check(due.silent >= 1, "the stalled block silenced");
    const auto extra = best(true);
    std::printf("extra block, deadline: longest block %.1f ms, silent %llu\n", extra.longest, (unsigned long long)extra.silent);
    check(extra.longest < 2.3 + 90, "with the extra block no block waits past the deadline either: " + std::to_string(extra.longest) + " ms");
    check(extra.silent >= 1, "the stalled block silenced, the part faded rather than waited for");
    unsetenv("OSCAIRN_WORKER_EXTRA_BLOCK");
    unsetenv("KICK_FIXTURE_STALL");

    // Step 6: two workers in a chain, each block 3 ms of work against a 1 ms allowance, a 100 ms deadline.
    setenv("KICK_FIXTURE_WORK_US", "3000", 1);
    setenv("OSCAIRN_WORKER_BUDGET_US", "1000", 1);
    const auto allowance = play(argv[2], true, 60, std::chrono::milliseconds(100));
    setenv("OSCAIRN_WORKER_DEADLINE", "1", 1);
    // The best of three, as above: a stall of the machine only adds misses.
    Run step6; step6.silent = UINT64_MAX;
    for(int i = 0; i < 3 && step6.silent + step6.late > 10; ++i)
        if(const auto r = play(argv[2], true, 60, std::chrono::milliseconds(100)); r.silent + r.late < step6.silent + step6.late || step6.silent == UINT64_MAX) step6 = r;
    std::printf("chain: allowance alone silent %llu late %llu; to the deadline silent %llu late %llu\n", (unsigned long long)allowance.silent,
                (unsigned long long)allowance.late, (unsigned long long)step6.silent, (unsigned long long)step6.late);
    check(allowance.silent + allowance.late >= 20, "the allowance alone misses the slow chain's blocks");
    check(step6.silent + step6.late <= 10, "waiting to the deadline, the chain plays");
    unsetenv("OSCAIRN_WORKER_DEADLINE");
    unsetenv("KICK_FIXTURE_WORK_US");
    setenv("OSCAIRN_WORKER_BUDGET_US", "50000", 1); // the deadline alone bounds every wait below

    // The extra block in real time, on the audio thread's priority.
    oscairn::worker::makeRealtime(1e6 * frames / rate, oscairn::worker::priority::engineAudio);
    const auto fewer = [](const Paced& a, const Paced& b) { return a.silent + uint64_t(a.over) < b.silent + uint64_t(b.over); };
    setenv("KICK_FIXTURE_WORK_US", "2400", 1);
    const auto near = bestOf(argv[2], 2000, fewer);
    std::printf("extra block, 2.4 ms blocks: silent %llu (busy %llu), callbacks past the period %d, longest %.2f ms, panics %llu\n", (unsigned long long)near.silent,
                (unsigned long long)near.busy, near.over, near.longest, (unsigned long long)near.panics);
    check(near.silent == 0, "the extra block: a 2.4 ms block every 2.667 ms period is never lost: " + std::to_string(near.silent));
    check(near.over == 0, "and no callback runs past the period: " + std::to_string(near.over));
    setenv("KICK_FIXTURE_WORK_US", "5500", 1);
    const auto shorter = [](const Paced& a, const Paced& b) { return a.longest < b.longest; };
    const auto slow = bestOf(argv[2], 400, shorter);
    std::printf("extra block, 5.5 ms blocks: silent %llu of 400, callbacks past the period %d, longest %.2f ms\n", (unsigned long long)slow.silent, slow.over, slow.longest);
    check(slow.silent >= 360, "5.5 ms blocks are lost, nearly every one (the first blocks after a too-slow restart are silent uncounted): " + std::to_string(slow.silent));
    // Waiting the block out would take 5.5 ms; the slack is for a virtual machine's late timers (a container's ran 1.3 ms past).
    check(slow.longest < 2.667 * (1 - oscairn::worker::mixingReserve) + 2.5, "and no callback waits past its deadline: " + std::to_string(slow.longest) + " ms");
    setenv("KICK_FIXTURE_WORK_US", "1000", 1);
    setenv("KICK_FIXTURE_STALL", "300:3000:0:1", 1); // the 300th block computes 3 ms more: 4 ms
    const auto four = bestOf(argv[2], 600, fewer);
    setenv("KICK_FIXTURE_STALL", "300:4000:0:1", 1); // 5 ms
    const auto five = bestOf(argv[2], 600, fewer);
    setenv("KICK_FIXTURE_STALL", "300:4500:0:1", 1); // 5.5 ms, past two periods
    const auto longer = bestOf(argv[2], 600, fewer);
    unsetenv("KICK_FIXTURE_STALL"); unsetenv("KICK_FIXTURE_WORK_US");
    std::printf("extra block, one long block among 1 ms ones: 4 ms lost %llu, 5 ms lost %llu (busy %llu), 5.5 ms lost %llu (busy %llu)\n", (unsigned long long)four.silent,
                (unsigned long long)five.silent, (unsigned long long)five.busy, (unsigned long long)longer.silent, (unsigned long long)longer.busy);
    check(four.silent == 0, "a 4 ms block among 1 ms ones fits before the next callback's deadline: lost " + std::to_string(four.silent));
    // At most: a virtual machine's late timers can give a block more than the deadline (exactly one and two on macOS).
    check(five.silent <= 1 && five.busy == 0, "a 5 ms block among 1 ms ones costs at most one block: lost " + std::to_string(five.silent) + ", busy " + std::to_string(five.busy));
    // Past two periods the worker is still in it when the next block is due: that one cannot be rendered either.
    check(longer.silent <= 2, "a 5.5 ms block costs at most the two blocks its worker is busy through: lost " + std::to_string(longer.silent));
    return failures ? 1 : 0;
}
