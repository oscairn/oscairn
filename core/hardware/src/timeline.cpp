// SPDX-License-Identifier: AGPL-3.0-or-later
#include <oscairn/timeline.h>
#include <algorithm>
#include <cmath>

namespace oscairn::hardware {

int64_t FakeClock::ticks(uint64_t sample) const noexcept {
    return offset + std::llround(double(sample) * ticksPerSample * (1 + driftPpm * 1e-6)) + (sample >= jumpAt ? jump : 0);
}

ClockMap::ClockMap(double nominalTicksPerSample, double jumpSamples) noexcept
    : nominal_(nominalTicksPerSample), jump_(jumpSamples), rate_(nominalTicksPerSample) {}

void ClockMap::reset() noexcept {
    count_ = next_ = 0;
    rate_ = nominal_; uncertainty_ = 0;
    ++generation_;
}

bool ClockMap::observe(uint64_t sample, int64_t ticks) noexcept {
    bool continuous = true;
    if (count_ && std::fabs(double(ticks) - (ticks0_ + (double(int64_t(sample - sample0_))) * rate_)) / rate_ > jump_) {
        reset();
        continuous = false;
    }
    samples_[next_] = sample; ticks_[next_] = ticks;
    next_ = (next_ + 1) % window;
    count_ = std::min(count_ + 1, window);
    fit();
    return continuous;
}

void ClockMap::fit() noexcept {
    // Relative to the oldest reading in the window, so the sums keep their precision.
    const uint32_t oldest = (next_ + window - count_) % window;
    const uint64_t s0 = samples_[oldest];
    const int64_t t0 = ticks_[oldest];
    if (count_ == 1) { sample0_ = s0; ticks0_ = double(t0); rate_ = nominal_; uncertainty_ = 0; return; }
    double mx = 0, my = 0;
    for (uint32_t i = 0; i < count_; ++i) {
        const uint32_t k = (oldest + i) % window;
        mx += double(int64_t(samples_[k] - s0)); my += double(ticks_[k] - t0);
    }
    mx /= count_; my /= count_;
    double sxy = 0, sxx = 0;
    for (uint32_t i = 0; i < count_; ++i) {
        const uint32_t k = (oldest + i) % window;
        const double x = double(int64_t(samples_[k] - s0)) - mx, y = double(ticks_[k] - t0) - my;
        sxy += x * y; sxx += x * x;
    }
    rate_ = sxx > 0 ? sxy / sxx : nominal_;
    if (!(rate_ > 0)) rate_ = nominal_;
    sample0_ = s0;
    ticks0_ = double(t0) + my - rate_ * mx;
    uncertainty_ = 0;
    for (uint32_t i = 0; i < count_; ++i) {
        const uint32_t k = (oldest + i) % window;
        const double predicted = ticks0_ + double(int64_t(samples_[k] - s0)) * rate_;
        uncertainty_ = std::max(uncertainty_, std::fabs(double(ticks_[k]) - predicted) / rate_);
    }
}

double ClockMap::sample(int64_t ticks) const noexcept { return double(sample0_) + (double(ticks) - ticks0_) / rate_; }
int64_t ClockMap::ticks(double sample) const noexcept { return std::llround(ticks0_ + (sample - double(sample0_)) * rate_); }
double ClockMap::driftPpm() const noexcept { return (rate_ / nominal_ - 1) * 1e6; }
double ClockMap::uncertainty() const noexcept { return uncertainty_; }

Placement place(const Input& in, const ClockMap& map, uint64_t start, uint32_t frames, uint32_t delay) noexcept {
    Placement p;
    p.origin = in.origin;
    p.uncertainty = map.uncertainty();
    const double at = map.sample(in.ticks) + delay;
    if (at < double(start)) {
        p.kind = Placement::late;
        p.lateBy = uint64_t(std::ceil(double(start) - at));
    } else if (at >= double(start + frames)) {
        p.kind = Placement::future;
    } else {
        p.offset = std::min<uint32_t>(uint32_t(at - double(start)), frames - 1);
    }
    return p;
}

void OutputQueue::insert(const Output& e) noexcept {
    uint32_t i = size_;
    while (i && (events_[i - 1].deadline > e.deadline || (events_[i - 1].deadline == e.deadline && events_[i - 1].order > e.order))) {
        events_[i] = events_[i - 1];
        --i;
    }
    events_[i] = e;
    ++size_;
    stats_.occupancy = size_;
    stats_.peak = std::max(stats_.peak, size_);
}

void OutputQueue::erase(uint32_t index) noexcept {
    for (uint32_t i = index + 1; i < size_; ++i) events_[i - 1] = events_[i];
    --size_;
}

void OutputQueue::hold(const Output& e) noexcept {
    if (e.kind == Output::data) return;
    const uint32_t k = (e.channel & 15) * 128 + (e.key & 127);
    held_[e.endpoint][k] = e.kind == Output::noteOn || e.kind == Output::gateOn;
    gates_[e.endpoint][k] = e.kind == Output::gateOn;
}

OutputQueue::Queued OutputQueue::push(Output e, uint64_t now) noexcept {
    if (e.endpoint >= maxEndpoints) return Queued::invalid;
    if (e.epoch != epoch_) { ++stats_.stale; return Queued::staleEpoch; }
    if (e.generation < generation_) { ++stats_.canceled; return Queued::canceled; }
    if (e.deadline < now + lead_) {
        ++stats_.late;
        if (!e.release()) return Queued::late;
        e.deadline = now + lead_;
    }
    const uint32_t free = capacity - size_;
    if (!free || (!e.release() && free <= reserve_)) { ++stats_.full; return Queued::full; }
    e.order = order_++;
    insert(e);
    ++stats_.queued;
    return Queued::ok;
}

uint64_t OutputQueue::cancel(uint32_t generation, uint64_t boundary, uint64_t now) noexcept {
    const uint64_t b = std::max(boundary, now + lead_);
    generation_ = std::max(generation_, generation);
    auto sameKey = [](const Output& a, const Output& b) { return a.endpoint == b.endpoint && a.channel == b.channel && a.key == b.key; };
    uint32_t kept = 0;
    for (uint32_t i = 0; i < size_; ++i) {
        Output e = events_[i];
        if (e.generation < generation && e.deadline >= b) {
            // A release whose note is on at the boundary (sent, or still queued before it) stays, at the boundary.
            bool on = e.release() && held(e.endpoint, e.channel, e.key);
            for (uint32_t j = 0; e.release() && !on && j < size_; ++j)
                on = events_[j].deadline < b && (events_[j].kind == Output::noteOn || events_[j].kind == Output::gateOn) && sameKey(events_[j], e);
            if (!on) { ++stats_.canceled; continue; }
            e.deadline = b; e.generation = generation;
        }
        events_[kept++] = e;
    }
    // Insertion again in order (std::stable_sort may allocate).
    size_ = 0;
    for (uint32_t i = 0; i < kept; ++i) { const Output e = events_[i]; insert(e); }
    return b;
}

} // namespace oscairn::hardware
