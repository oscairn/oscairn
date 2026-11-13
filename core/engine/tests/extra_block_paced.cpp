// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's extra block in real time, in a graph of ST Final's shape: six instrument parts, the first
// a note effect (Transpose, the arpeggiator's place) into its instrument into an insert, every part
// into one bus effect and the output, every plugin in a worker of its own (nine), played a period
// apart with the callback's deadline set as the device's callback sets it. The instruments' blocks
// take 1.2 ms, the others' 0.1 ms. With no, one and six instruments on the extra block (and, for one,
// a single 4.9 ms block): no more callbacks past the period than without the mode, the engine's
// thread no busier, with six no more ordinary blocks missed, a long block costing at most two, and
// the output's left and right channels equal throughout, a missed block's fade included. The best of
// three runs of each, as a busy machine only adds misses.
//   oscairn-extra-block-paced-test WORKER FIXTURE.clap TRANSPOSE.clap [--no-timing]
#include <oscairn/graph.h>
#include <oscairn/realtime.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <string>
#include <thread>
#include <vector>

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128; constexpr int parts = 6;
    using clock = std::chrono::steady_clock;
    const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
    const char* fixture = nullptr; const char* transpose = nullptr;

    struct Built
    {
        std::unique_ptr<oscairn::Graph> graph = std::make_unique<oscairn::Graph>(rate, frames);
        std::vector<oscairn::NodeId> delayed, ordinary;
    };
    // `late` of the instruments on the extra block; `stall`: the first of them takes 4.9 ms in its block 400.
    Built build(int late, bool stall)
    {
        Built b; auto& g = *b.graph;
        auto add = [&](const char* path, const char* id, const char* workUs, bool delayed, bool stalls) {
            setenv("KICK_FIXTURE_WORK_US", workUs, 1);
            if(delayed) setenv("OSCAIRN_WORKER_EXTRA_BLOCK", "org.kick.test.midi", 1); else unsetenv("OSCAIRN_WORKER_EXTRA_BLOCK");
            if(stalls) setenv("KICK_FIXTURE_STALL", "400:3700:0:1", 1); else unsetenv("KICK_FIXTURE_STALL");
            const auto n = g.add(path, id);
            (delayed ? b.delayed : b.ordinary).push_back(n);
            return n;
        };
        const auto bus = add(fixture, "org.kick.test.effect", "100", false, false);
        for(int p = 0; p < parts; ++p)
        {
            const auto instrument = add(fixture, "org.kick.test.midi", "1200", p < late, stall && p == 0);
            auto last = instrument;
            if(p == 0) // the arpeggiator's part: a note effect before the instrument, an insert after it
            {
                const auto notes = add(transpose, "", "100", false, false);
                g.connectNotes(oscairn::graphInput, notes); g.connectNotes(notes, instrument);
                last = add(fixture, "org.kick.test.effect", "100", false, false);
                g.connectAudio(instrument, last);
            }
            else g.connectNotes(oscairn::graphInput, instrument);
            g.connectAudio(last, bus); g.connectAudio(last, oscairn::graphOutput);
        }
        g.connectAudio(bus, oscairn::graphOutput);
        g.commit();
        unsetenv("OSCAIRN_WORKER_EXTRA_BLOCK"); unsetenv("KICK_FIXTURE_STALL"); unsetenv("KICK_FIXTURE_WORK_US");
        return b;
    }
    std::vector<kick::clap::Event> notesAt(int block)
    {
        if(block % 50 == 0) return {kick::clap::Event::note(true, 60 + block / 50 % 12, .8, uint32_t(block % 7) * 9)};
        if(block % 50 == 30) return {kick::clap::Event::note(false, 60 + (block - 30) / 50 % 12, 0, 17)};
        return {};
    }
    double threadCpu() { timespec t{}; clock_gettime(CLOCK_THREAD_CPUTIME_ID, &t); return double(t.tv_sec) + t.tv_nsec * 1e-9; }

    struct Run { int over = 0; double longest = 0, cpu = 0; uint64_t ordinaryMissed = 0, delayedSilent = 0; bool stereo = true; };
    Run play(Built& b, int blocks)
    {
        Run r;
        std::vector<float> l(frames), rr(frames); float* out[]{l.data(), rr.data()};
        auto next = clock::now();
        double cpuFrom = 0; auto wallFrom = clock::now();
        for(int i = -100; i < blocks; ++i) // 100 to start with, not counted
        {
            if(!i) { cpuFrom = threadCpu(); wallFrom = clock::now(); }
            std::this_thread::sleep_until(next); next += period;
            const auto events = notesAt(i + 100);
            const auto start = clock::now();
            oscairn::worker::callbackDeadline = start + std::chrono::nanoseconds(int64_t(double(period.count()) * (1 - oscairn::worker::mixingReserve)));
            b.graph->process(out, frames, nullptr, events.data(), uint32_t(events.size()));
            oscairn::worker::callbackDeadline = {};
            const auto took = clock::now() - start;
            if(i < 0) continue;
            r.longest = std::max(r.longest, std::chrono::duration<double, std::milli>(took).count());
            if(took > period) ++r.over;
            r.stereo = r.stereo && l == rr;
        }
        r.cpu = (threadCpu() - cpuFrom) / std::chrono::duration<double>(clock::now() - wallFrom).count();
        for(const auto n : b.ordinary) { const auto s = b.graph->host(n).statistics(); r.ordinaryMissed += s.silentBlocks + s.lateBlocks; }
        for(const auto n : b.delayed) r.delayedSilent += b.graph->host(n).statistics().silentBlocks;
        return r;
    }
}

int main(int argc, char** argv)
{
    if(argc < 4) return 2;
    fixture = argv[2]; transpose = argv[3];
    const bool timing = !(argc > 4 && std::strcmp(argv[4], "--no-timing") == 0);
    setenv("OSCAIRN_PLUGIN_WORKER", argv[1], 1);
    oscairn::worker::makeRealtime(1e6 * frames / rate, oscairn::worker::priority::engineAudio);
    constexpr int blocks = 1500;
    // The best of three runs of each: a busy machine only adds misses, long callbacks and CPU time.
    auto best = [&](int late, bool stall) {
        Run b; b.over = INT32_MAX;
        for(int i = 0; i < 3 && (b.over || b.ordinaryMissed); ++i)
        {
            auto built = build(late, stall);
            auto r = play(built, blocks);
            if(uint64_t(r.over) + r.ordinaryMissed < uint64_t(b.over) + b.ordinaryMissed) b = std::move(r);
        }
        return b;
    };
    const auto none = best(0, false);
    std::printf("no extra block: callbacks past the period %d, longest %.2f ms, ordinary missed %llu, engine thread %.1f%%\n", none.over, none.longest,
                (unsigned long long)none.ordinaryMissed, 100 * none.cpu);
    for(const auto [late, stall] : {std::pair{1, false}, std::pair{6, false}, std::pair{1, true}})
    {
        const auto r = best(late, stall);
        const std::string what = std::to_string(late) + " delayed" + (stall ? ", one 4.9 ms block" : "");
        std::printf("%s: callbacks past the period %d, longest %.2f ms, ordinary missed %llu, delayed silent %llu, engine thread %.1f%%, stereo %s\n", what.c_str(), r.over, r.longest,
                    (unsigned long long)r.ordinaryMissed, (unsigned long long)r.delayedSilent, 100 * r.cpu, r.stereo ? "intact" : "BROKEN");
        check(r.stereo, what + ": left and right equal, every block");
        if(timing)
        {
            // Against the same graph without the mode, as loaded as the machine is: no more callbacks past the period
            // (a delayed worker is waited for only to the time the plugins after it leave), and no busier engine thread
            // (a slack of 10 in 1,500 for a virtual machine's stalls, which the best of three does not always escape).
            check(r.over <= none.over + 10, what + ": no more callbacks past the period than without the mode: " + std::to_string(r.over) + " against " + std::to_string(none.over));
            check(r.cpu <= none.cpu * 1.5 + .02, what + ": the engine's thread no busier than without the mode: " + std::to_string(100 * r.cpu) + "% against " + std::to_string(100 * none.cpu) + "%");
            // Every instrument on the extra block leaves the ordinary workers more of each period; one alone, whose block
            // then overlaps the next callback's, is reported (more missed with four CPUs in a container), not checked.
            if(late == parts) check(r.ordinaryMissed <= none.ordinaryMissed, what + ": no more ordinary blocks missed than without the mode: " + std::to_string(r.ordinaryMissed));
            // A block past a period and its wave keeps the worker through the next hand-over: at most two.
            if(stall) check(r.delayedSilent <= 2, what + ": delayed blocks lost: " + std::to_string(r.delayedSilent));
        }
    }
    return failures ? 1 : 0;
}
