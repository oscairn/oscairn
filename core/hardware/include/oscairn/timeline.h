// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// One event timeline with explicit device clocks (docs/rfcs/0004-hardware-services.md#one-event-timeline-explicit-device-clocks).
//
// The timeline is the audio sample counter, which restarts in a new epoch when the device or
// session timeline resets (an audio-device restart). Each device clock is mapped onto it by an
// offset and a rate estimated from paired readings, with its drift and uncertainty reported.
// Inputs keep their device timestamps to graph ingress; outputs carry their deadline, epoch and
// cancellation generation, and reach the device a lead time ahead. Nothing here allocates, locks
// or waits: every call may be made on the audio thread.
#include <array>
#include <cstdint>
#include <limits>

namespace oscairn::hardware {

// A device clock for tests: its ticks at an audio sample, nominal rate scaled by its drift, plus
// an offset, and from jumpAt on a jump (a device's clock reset or stepped).
struct FakeClock {
    double ticksPerSample = 1;
    double driftPpm = 0;
    int64_t offset = 0;
    uint64_t jumpAt = std::numeric_limits<uint64_t>::max();
    int64_t jump = 0;
    int64_t ticks(uint64_t sample) const noexcept;
};

// The mapping from a device clock onto the timeline, fitted to its recent paired readings (both
// clocks read together, as at a block boundary) by least squares. A reading off the fitted line by
// more than jumpSamples is a jump: the fit restarts from it and generation() increases.
class ClockMap {
public:
    static constexpr uint32_t window = 32;
    explicit ClockMap(double nominalTicksPerSample, double jumpSamples = 64) noexcept;
    // False when the reading was a jump.
    bool observe(uint64_t sample, int64_t ticks) noexcept;
    bool valid() const noexcept { return count_ >= 2; }
    double sample(int64_t ticks) const noexcept;   // a device time on the timeline
    int64_t ticks(double sample) const noexcept;   // a timeline position in device ticks
    double driftPpm() const noexcept;              // the fitted rate's departure from nominal
    double uncertainty() const noexcept;           // samples: the largest residual in the window
    uint32_t generation() const noexcept { return generation_; }
    void reset() noexcept;                         // forget every reading (a reconnect); generation increases

private:
    void fit() noexcept;
    double nominal_, jump_;
    std::array<uint64_t, window> samples_{};
    std::array<int64_t, window> ticks_{};
    uint32_t count_ = 0, next_ = 0, generation_ = 0;
    double rate_ = 0, uncertainty_ = 0;            // ticks per sample, fitted
    uint64_t sample0_ = 0;                         // the fit's reference point
    double ticks0_ = 0;
};

// An input's time: a capture timestamp from the device, or, where it gives none, the earliest
// time it was received, never presented as a capture time.
enum class Origin : uint8_t { capture, receive };
struct Input {
    uint32_t source = 0, sequence = 0;
    int64_t ticks = 0;
    Origin origin = Origin::capture;
};
// Where an input lands in the block [start, start + frames): delay samples after its time on the
// timeline (the latency policy). Late inputs cannot change audio already played: the caller
// applies them at offset 0 or discards them by their type, and reports lateBy.
struct Placement {
    enum Kind : uint8_t { inBlock, late, future } kind = inBlock;
    uint32_t offset = 0;
    uint64_t lateBy = 0;
    double uncertainty = 0; // samples: the clock map's, and the input's origin says what it is of
    Origin origin = Origin::capture;
};
Placement place(const Input&, const ClockMap&, uint64_t start, uint32_t frames, uint32_t delay) noexcept;

// An event for a device, at its presentation deadline on the timeline.
struct Output {
    enum Kind : uint8_t { noteOn, noteOff, gateOn, gateOff, data };
    uint32_t endpoint = 0;  // < OutputQueue::maxEndpoints
    uint64_t deadline = 0;
    uint32_t epoch = 0, generation = 0;
    Kind kind = data;
    uint8_t channel = 0, key = 0, value = 0;
    uint64_t order = 0;     // set by the queue: equal deadlines go in the order they were queued
    bool release() const noexcept { return kind == noteOff || kind == gateOff; }
};

// The bounded queue of a device's scheduled outputs, ordered by deadline and then by the order
// they were queued. A slice of it is kept for releases (note and gate offs), so a full queue
// still takes them; a late release is sent as soon as it can be, never dropped. It mirrors which
// notes and gates the device holds, for cancellation and for the releases a lost link needs.
class OutputQueue {
public:
    static constexpr uint32_t capacity = 256, maxEndpoints = 8;
    enum class Queued { ok, full, late, staleEpoch, canceled, invalid };
    struct Stats { uint64_t queued = 0, sent = 0, full = 0, late = 0, stale = 0, canceled = 0; uint32_t occupancy = 0, peak = 0; };

    // lead: how far ahead of its deadline an event must reach the device; reserve: slots kept for releases.
    OutputQueue(uint32_t lead, uint32_t reserve = 16) noexcept : lead_(lead), reserve_(reserve) {}
    // now: the timeline position the device has been sent up to. A late release is queued at now + lead.
    Queued push(Output, uint64_t now) noexcept;
    // Stop, seek, a tempo change or an edit: events not yet sent with a generation below generation
    // and a deadline at or after the boundary are dropped. The boundary is at least now + lead, since
    // what is sent cannot be recalled. A note or gate already on keeps its release, moved to the
    // boundary. Returns the boundary used. Later events must carry the new generation.
    uint64_t cancel(uint32_t generation, uint64_t boundary, uint64_t now) noexcept;
    // Hands over every event due by until (now + lead), in order, to send(const Output&).
    template<class F> uint32_t send(uint64_t until, F&& send) noexcept {
        uint32_t n = 0;
        while (size_ && events_[0].deadline <= until) {
            const Output e = events_[0];
            erase(0);
            hold(e);
            send(e);
            ++stats_.sent; ++n;
        }
        stats_.occupancy = size_;
        return n;
    }
    // The link to the device is lost: drops every queued event and hands over a release for each
    // note and gate the device was sent and not released, for the link's return (the device's own
    // watchdog releases them meanwhile). Later events must carry epoch, the new one.
    template<class F> uint32_t disconnect(uint32_t epoch, F&& release) noexcept {
        size_ = 0; stats_.occupancy = 0; epoch_ = epoch;
        uint32_t n = 0;
        for (uint32_t e = 0; e < maxEndpoints; ++e)
            for (uint32_t i = 0; i < 16 * 128; ++i)
                if (held_[e][i]) {
                    Output r; r.endpoint = e; r.channel = uint8_t(i / 128); r.key = uint8_t(i % 128); r.epoch = epoch;
                    r.kind = gates_[e][i] ? Output::gateOff : Output::noteOff;
                    held_[e][i] = gates_[e][i] = false;
                    release(r); ++n;
                }
        return n;
    }
    uint32_t size() const noexcept { return size_; }
    uint32_t epoch() const noexcept { return epoch_; }
    uint32_t generation() const noexcept { return generation_; }
    const Stats& stats() const noexcept { return stats_; }
    bool held(uint32_t endpoint, uint8_t channel, uint8_t key) const noexcept { return endpoint < maxEndpoints && held_[endpoint][(channel & 15) * 128 + (key & 127)]; }

private:
    void insert(const Output&) noexcept;
    void erase(uint32_t index) noexcept;
    void hold(const Output&) noexcept;
    std::array<Output, capacity> events_{};
    uint32_t size_ = 0, lead_, reserve_, epoch_ = 0, generation_ = 0;
    uint64_t order_ = 0;
    bool held_[maxEndpoints][16 * 128] = {};  // a note or gate on at the device
    bool gates_[maxEndpoints][16 * 128] = {}; // held as a gate, not a note
    Stats stats_;
};

} // namespace oscairn::hardware
