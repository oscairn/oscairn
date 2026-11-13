// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A Performance of ST Final's shape, every plugin in its own worker, played in real time for a
// while and timed as engine.timing sees it (RFC 0045): six instrument parts (a note held on each),
// the sixth through an insert effect, and three effect buses every part sends to, so the longest
// chain is three workers one after another (instrument, insert, bus effect) and the widest wave six.
// Each plugin computes KICK_FIXTURE_WORK_US a block (150 unless set). For comparing the scheduling
// settings (OSCAIRN_WORKER_SPIN_US, OSCAIRN_PLUGIN_PLACEMENT, OSCAIRN_WORKER_DEADLINE, ...) on one
// machine; absolute times on another machine, or in a container, do not transfer.
// SHAPE small is Rd 73 Chill's instead: one instrument through an insert, no buses.
//   oscairn-worker-bench WORKER FIXTURE.clap [SECONDS [SHAPE]]
#include <oscairn/session.h>
#include <oscairn/realtime.h>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>
#include <vector>

int main(int argc, char** argv)
{
    if(argc < 3) return 2;
    const double seconds = argc > 3 ? std::atof(argv[3]) : 5;
    const bool small = argc > 4 && std::string(argv[4]) == "small";
    setenv("OSCAIRN_PLUGIN_WORKER", argv[1], 0);
    setenv("KICK_FIXTURE_WORK_US", "150", 0);
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
    oscairn::Session session(rate, frames);
    std::vector<uint32_t> buses;
    for(int b = 0; b < (small ? 0 : 3); ++b)
    {
        buses.push_back(session.addPart("Bus " + std::to_string(b + 1), {oscairn::PartInput::Kind::Bus}));
        session.insertIntoPart(buses.back(), 0, session.add("Send effect " + std::to_string(b + 1), argv[2], "org.kick.test.effect"));
    }
    const int parts = small ? 1 : 6;
    for(int p = 0; p < parts; ++p)
    {
        const auto part = session.addPart("Part " + std::to_string(p + 1));
        session.insertIntoPart(part, 0, session.add("Instrument " + std::to_string(p + 1), argv[2], "org.kick.test.synth"));
        if(p == parts - 1) session.insertIntoPart(part, 1, session.add("Insert", argv[2], "org.kick.test.effect"));
        for(const auto bus : buses) session.setSend(part, bus, .2);
    }
    session.commit();
    float l[frames], r[frames]; float* out[]{l, r};
    oscairn::worker::makeRealtime(1e6 * frames / rate, oscairn::worker::priority::engineAudio); // as the device's thread; may be refused off a device
    auto next = std::chrono::steady_clock::now();
    auto play = [&](double s, bool notes) {
        const auto blocks = int64_t(s * rate / frames);
        for(int64_t i = 0; i < blocks; ++i)
        {
            std::this_thread::sleep_until(next);
            const auto start = std::chrono::steady_clock::now();
            std::vector<kick::clap::Event> e;
            if(notes && i == 0) e.push_back(kick::clap::Event::note(true, 60, .8));
            session.process(out, frames, e);
            const auto end = std::chrono::steady_clock::now();
            session.blockTimes(start.time_since_epoch().count(), end.time_since_epoch().count(), period.count());
            next += period;
            if(end > next) next = end; // behind: start again from now, as a device would after an underrun
            if(i % 64 == 0) session.service();
        }
    };
    play(1, true); // the warm-up: workers' first blocks, a note on every part
    session.engineTiming(true);
    play(seconds, false);
    const auto t = session.engineTiming(false);
    auto us = [](const oscairn::Histogram& h, double q) { return h.percentileMicroseconds(q); };
    std::printf("%-16s %8s %8s %8s %8s %8s %8s %8s %6s %6s %6s\n", "worker", "rt p50", "rt p99", "rt max", "wake p50", "wake p99", "run p50", "cpu p50", "late", "silent", "busy");
    uint64_t late = 0, silent = 0;
    for(const auto& w : t.workers)
    {
        const auto& h = w.roundTrip.steady;
        std::printf("%-16s %8.0f %8.0f %8.0f %8.0f %8.0f %8.0f %8.0f %6llu %6llu %6llu\n", w.name.c_str(), us(h, .5), us(h, .99), h.maxNs / 1e3, us(w.wake.steady, .5), us(w.wake.steady, .99),
                    us(w.run.steady, .5), us(w.cpu.steady, .5), (unsigned long long)w.late, (unsigned long long)w.silent, (unsigned long long)w.busy);
        late += w.late; silent += w.silent;
    }
    const auto& c = t.callback.steady;
    std::printf("callback p50 %.0f p99 %.0f max %.0f us over %llu blocks; workers %zu; blocks late %llu, silent %llu\n", us(c, .5), us(c, .99), c.maxNs / 1e3,
                (unsigned long long)c.count(), t.workers.size(), (unsigned long long)late, (unsigned long long)silent);
    // The buckets are powers of two: the share of callbacks past 1024 us says more than a percentile's bucket top.
    uint64_t over = 0; for(size_t i = 7; i < c.buckets; ++i) over += c.counts[i];
    std::printf("SUMMARY workers=%zu callback_p50=%.0f callback_p99=%.0f callback_max=%.0f callback_over_1024us=%.2f%% late=%llu silent=%llu\n", t.workers.size(), us(c, .5), us(c, .99), c.maxNs / 1e3,
                c.count() ? 100.0 * double(over) / double(c.count()) : 0.0, (unsigned long long)late, (unsigned long long)silent);
    // A run, not a judgment: only that every plugin played in a worker and was timed.
    return t.workers.size() == (small ? 2u : 10u) && c.count() > 0 ? 0 : 1;
}
