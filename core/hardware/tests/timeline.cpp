// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0004 step 1, deterministically: a device clock's offset, drift and jump mapped onto the audio
// timeline; input timestamps placed in blocks, and late input; scheduled outputs' equal-time
// ordering, lead time, overflow, cancellation, a lost link's safe releases and the reconnect.
#include <oscairn/timeline.h>
#include <cmath>
#include <cstdlib>
#include <cstdio>
#include <vector>

using namespace oscairn::hardware;

namespace {
int failures = 0;
void expect(bool ok, const char* what) {
    std::printf("%s: %s\n", ok ? "ok" : "FAIL", what);
    if (!ok) ++failures;
}

// Feeds the map a reading at every 128-frame block boundary in [from, to); the largest error of
// the mapping over that span afterward, in samples.
double track(ClockMap& map, const FakeClock& clock, uint64_t from, uint64_t to, uint32_t* jumps = nullptr) {
    for (uint64_t s = from; s < to; s += 128)
        if (!map.observe(s, clock.ticks(s)) && jumps) ++*jumps;
    double worst = 0;
    for (uint64_t s = from; s < to; s += 97) worst = std::max(worst, std::fabs(map.sample(clock.ticks(s)) - double(s)));
    return worst;
}
} // namespace

int main() {
    // A 48 MHz device timer against 48 kHz audio: 1000 ticks a sample.
    {
        FakeClock clock{1000, 50, 123456789};
        ClockMap map(1000);
        const double error = track(map, clock, 0, 48000 * 10);
        std::printf("offset 123456789 ticks, +50 ppm: drift %.4f ppm, uncertainty %.5f samples, error %.5f samples\n",
                    map.driftPpm(), map.uncertainty(), error);
        expect(std::fabs(map.driftPpm() - 50) < 0.1, "a +50 ppm drift is estimated within 0.1 ppm (32 readings, 85 ms apart at the ends)");
        expect(error < 0.01, "an offset device clock maps onto the timeline within 0.01 samples");
        expect(map.uncertainty() < 0.01, "the reported uncertainty is the residual: under 0.01 samples");
        // A second ahead of the last reading, the fitted rate carries the mapping.
        const uint64_t ahead = 48000 * 11;
        expect(std::fabs(map.sample(clock.ticks(ahead)) - double(ahead)) < 0.05, "a second past the readings maps within 0.05 samples");
        expect(std::llabs(map.ticks(double(ahead)) - clock.ticks(ahead)) <= 50, "and back to device ticks within 50 ticks (0.05 samples)");
    }
    {
        FakeClock clock{1000, -120, -987654321};
        ClockMap map(1000);
        const double error = track(map, clock, 0, 48000 * 10);
        std::printf("offset -987654321 ticks, -120 ppm: drift %.4f ppm, error %.5f samples\n", map.driftPpm(), error);
        expect(std::fabs(map.driftPpm() + 120) < 0.1 && error < 0.01, "a negative offset and -120 ppm map as well");
    }
    // A clock that jumps 5 ms (240 samples) at sample 240000: the map restarts there.
    {
        FakeClock clock{1000, 30, 1000, 240000, 240 * 1000};
        ClockMap map(1000);
        uint32_t jumps = 0;
        track(map, clock, 0, 240000, &jumps);
        const uint32_t before = map.generation();
        const double error = track(map, clock, 240000, 480000, &jumps);
        std::printf("a 240-sample jump: %u jump(s) found, generation %u -> %u, error after %.5f samples, drift %.4f ppm\n",
                    jumps, before, map.generation(), error, map.driftPpm());
        expect(jumps == 1 && map.generation() == before + 1, "the jump is found once and starts a new generation");
        expect(error < 0.01 && std::fabs(map.driftPpm() - 30) < 0.1, "after it the clock maps again within 0.01 samples");
    }

    // Input timestamps, 64 samples of latency policy, blocks of 128.
    {
        FakeClock clock{1000, 0, 5000000};
        ClockMap map(1000);
        track(map, clock, 0, 4800);
        Input in{1, 7, clock.ticks(1000), Origin::capture};
        auto p = place(in, map, 1024, 128, 64); // at 1064
        expect(p.kind == Placement::inBlock && p.offset == 40, "a capture at sample 1000 lands at offset 40 of the block at 1024, 64 later");
        p = place(in, map, 1152, 128, 64);
        expect(p.kind == Placement::late && p.lateBy == 88, "the same input for the block at 1152 is late by 88 samples");
        p = place(in, map, 896, 128, 64);
        expect(p.kind == Placement::future, "and for the block at 896 it is not due yet");
        in.origin = Origin::receive;
        expect(place(in, map, 1024, 128, 64).origin == Origin::receive, "a receive time stays a receive time, never a capture time");
    }

    // Scheduled outputs: 32 samples of lead, the reserve for releases 16.
    {
        OutputQueue q(32);
        std::vector<Output> sent;
        auto collect = [&](const Output& e) { sent.push_back(e); };
        auto note = [](Output::Kind k, uint64_t at, uint8_t key, uint32_t gen = 0) { Output e; e.kind = k; e.deadline = at; e.key = key; e.generation = gen; return e; };
        // Equal deadlines go in the order they were queued; earlier deadlines first.
        q.push(note(Output::noteOn, 500, 60), 0);
        q.push(note(Output::noteOn, 400, 61), 0);
        q.push(note(Output::noteOn, 500, 62), 0);
        q.push(note(Output::noteOn, 500, 63), 0);
        q.send(500, collect);
        expect(sent.size() == 4 && sent[0].key == 61 && sent[1].key == 60 && sent[2].key == 62 && sent[3].key == 63,
               "deadline order, then queued order for equal deadlines (61, 60, 62, 63)");
        // Lead time: what cannot reach the device in time is refused, except a release, sent as soon as it can be.
        expect(q.push(note(Output::noteOn, 520, 64), 500) == OutputQueue::Queued::late, "a note-on inside the lead time is refused as late");
        expect(q.push(note(Output::noteOff, 510, 60), 500) == OutputQueue::Queued::ok && q.size() == 1, "a late note-off is still queued");
        sent.clear(); q.send(532, collect);
        expect(sent.size() == 1 && sent[0].deadline == 532, "and sent at now + lead (532)");
        expect(q.stats().late == 2, "both counted late");
        // Overflow: the last 16 slots take releases only.
        uint32_t taken = 0;
        while (q.push(note(Output::data, 10000 + taken, 0), 600) == OutputQueue::Queued::ok) ++taken;
        expect(taken == OutputQueue::capacity - 16, "240 ordinary events fill the queue but its release reserve");
        uint32_t releases = 0;
        while (q.push(note(Output::noteOff, 20000, 1), 600) == OutputQueue::Queued::ok) ++releases;
        expect(releases == 16 && q.stats().full == 2 && q.stats().peak == OutputQueue::capacity,
               "16 releases still fit; the overflow is counted (2 refused) and the peak is 256");
    }
    // Cancellation from a boundary: future notes go; a note already sounding keeps its release, at the boundary.
    {
        OutputQueue q(32);
        std::vector<Output> sent;
        auto collect = [&](const Output& e) { sent.push_back(e); };
        auto note = [](Output::Kind k, uint64_t at, uint8_t key, uint32_t gen = 0) { Output e; e.kind = k; e.deadline = at; e.key = key; e.generation = gen; return e; };
        q.push(note(Output::noteOn, 100, 60), 0);
        q.push(note(Output::noteOff, 900, 60), 0);   // 60 sounds when the stop comes
        q.push(note(Output::noteOn, 600, 62), 0);    // queued, not yet sent
        q.push(note(Output::noteOff, 800, 62), 0);
        q.push(note(Output::noteOn, 200, 64), 0);    // its off comes before the boundary: untouched
        q.push(note(Output::noteOff, 300, 64), 0);
        q.push(note(Output::noteOn, 420, 65), 0);    // queued before the boundary: its off moves to it
        q.push(note(Output::noteOff, 700, 65), 0);
        q.send(132, collect);                         // up to 100 + lead: note 60 is on at the device
        expect(sent.size() == 1 && q.held(0, 0, 60), "note 60 is sent and held");
        const uint64_t at = q.cancel(1, 160, 400);    // asked for 160, but 400 + 32 is the earliest
        expect(at == 432, "the cancellation takes effect at now + lead (432), not before");
        sent.clear(); q.send(2000, collect);
        bool off60 = false, on62 = false, off65 = false;
        for (const auto& e : sent) {
            off60 |= e.key == 60 && e.kind == Output::noteOff && e.deadline == 432;
            on62 |= e.key == 62;
            off65 |= e.key == 65 && e.kind == Output::noteOff && e.deadline == 432;
        }
        expect(off60, "the sounding note 60 is released at the boundary");
        expect(!on62, "the future note 62 and its release are gone");
        expect(off65, "note 65, queued before the boundary, is released at it");
        expect(!q.held(0, 0, 60) && !q.held(0, 0, 64) && !q.held(0, 0, 65), "nothing is left held");
        expect(q.push(note(Output::noteOn, 3000, 66, 0), 2000) == OutputQueue::Queued::canceled, "an event of the old generation is refused");
        expect(q.push(note(Output::noteOn, 3000, 66, 1), 2000) == OutputQueue::Queued::ok, "one of the new generation is queued");
    }
    // The link is lost with a note and a gate held: releases for both, nothing stale kept; then the reconnect.
    {
        OutputQueue q(32);
        std::vector<Output> sent, released;
        auto note = [](Output::Kind k, uint64_t at, uint8_t key, uint32_t endpoint = 0) { Output e; e.kind = k; e.deadline = at; e.key = key; e.endpoint = endpoint; return e; };
        q.push(note(Output::noteOn, 100, 48), 0);
        q.push(note(Output::gateOn, 100, 0, 3), 0);
        q.push(note(Output::noteOn, 900, 50), 0);
        q.send(200, [&](const Output& e) { sent.push_back(e); });
        const uint32_t n = q.disconnect(1, [&](const Output& e) { released.push_back(e); });
        bool note48 = false, gate = false;
        for (const auto& e : released) {
            note48 |= e.kind == Output::noteOff && e.key == 48 && e.endpoint == 0;
            gate |= e.kind == Output::gateOff && e.endpoint == 3;
        }
        expect(n == 2 && note48 && gate, "a lost link gives a release for the held note and the high gate");
        expect(q.size() == 0, "the unsent note 50 is dropped, not played after the reconnect");
        expect(q.push(note(Output::noteOn, 1000, 52), 300) == OutputQueue::Queued::staleEpoch, "an event of the old epoch is refused");
        Output fresh = note(Output::noteOn, 1000, 52); fresh.epoch = 1;
        expect(q.push(fresh, 300) == OutputQueue::Queued::ok, "one of the new epoch is queued");
        ClockMap map(1000);
        FakeClock clock{1000, 0, 42};
        track(map, clock, 0, 4800);
        const uint32_t generation = map.generation();
        map.reset();
        FakeClock after{1000, 0, 777777};
        const double error = track(map, after, 4800, 9600);
        expect(map.generation() == generation + 1 && error < 0.01, "the reconnected device's clock is mapped afresh, in a new generation");
    }

    std::printf("%s\n", failures ? "FAIL" : "PASS");
    return failures ? 1 : 0;
}
