// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Timing histograms for the audio path (engine.timing in the control protocol): recorded by one
// thread at a time, the audio thread or a graph helper rendering for it, with no allocation, lock
// or system call; read, reset and warmed up from any thread.
#pragma once
#include <algorithm>
#include <array>
#include <atomic>
#include <bit>
#include <cmath>
#include <cstddef>
#include <cstdint>

namespace oscairn
{
    // One measurement's distribution: buckets by powers of two, bucket 0 below 16 µs, bucket i
    // (1 to 12) from 16·2^(i-1) up to 16·2^i µs, the last (13) at or above 65.536 ms; the largest
    // value and when it was, in nanoseconds, its time counted from the start of the window.
    struct Histogram
    {
        static constexpr size_t buckets = 14;
        // The bucket's upper bound in microseconds; the last bucket has none.
        static constexpr uint64_t upperMicroseconds(size_t i) noexcept { return uint64_t(16) << i; }
        static constexpr size_t bucket(int64_t ns) noexcept
        {
            return std::min<size_t>(size_t(std::bit_width(ns <= 0 ? uint64_t(0) : uint64_t(ns) / 16000)), buckets - 1);
        }
        std::array<uint64_t, buckets> counts{};
        int64_t maxNs = 0, maxAt = 0;
        uint64_t count() const noexcept { uint64_t n = 0; for(auto c : counts) n += c; return n; }
        // The value at or below which the fraction q of the values lie, in microseconds: the upper
        // bound of the bucket that holds it, no more than the largest value; 0 when empty.
        double percentileMicroseconds(double q) const noexcept
        {
            const auto n = count();
            if(!n) return 0;
            const auto rank = std::max<uint64_t>(1, uint64_t(std::ceil(q * double(n))));
            uint64_t seen = 0;
            for(size_t i = 0; i < buckets; ++i)
                if((seen += counts[i]) >= rank) return i + 1 < buckets ? std::min(double(upperMicroseconds(i)), maxNs / 1e3) : maxNs / 1e3;
            return maxNs / 1e3;
        }
    };

    // A measurement taken once per block, kept in two histograms since the last reset: startup, for
    // the first warmupBlocks blocks after the recorder is made or warmUp() is called (a device start,
    // a Performance load, a worker's restart), and steady, for every block after. One-offs at a start
    // (first-touch page faults, a plugin's lazy first-block work, a worker's first wake-ups, a
    // frequency governor ramping up) stay out of the steady histogram.
    class BlockTiming
    {
    public:
        // 512 blocks: 1.4 s at 128 frames and 48 kHz, 5.5 s at 512; long enough for the one-offs
        // above, short beside a measurement of tens of seconds.
        static constexpr uint32_t warmupBlocks = 512;
        struct Snapshot { Histogram steady, startup; double seconds = 0; }; // seconds: from the window's first block to its last

        // The recording thread. `at`: when, in nanoseconds on its clock.
        void record(int64_t ns, int64_t at) noexcept
        {
            if(const auto r = resets_.load(std::memory_order_acquire); r != resetDone_.load(std::memory_order_relaxed))
            {
                steady_.clear(); startup_.clear(); origin_ = at; started_ = true;
                resetDone_.store(r, std::memory_order_release);
            }
            if(const auto w = warmups_.load(std::memory_order_relaxed); w != warmupSeen_) { warmupSeen_ = w; warmupLeft_ = warmupBlocks; }
            if(!started_) { origin_ = at; started_ = true; }
            (warmupLeft_ ? (--warmupLeft_, startup_) : steady_).add(ns, at - origin_);
            last_.store(at - origin_, std::memory_order_relaxed);
        }
        // Any thread: what the window holds (empty while a reset waits for the next block).
        Snapshot read() const noexcept
        {
            if(resets_.load(std::memory_order_acquire) != resetDone_.load(std::memory_order_acquire)) return {};
            return {steady_.read(), startup_.read(), last_.load(std::memory_order_relaxed) / 1e9};
        }
        // Any thread: a new window from the next block; the recording thread clears it then.
        void reset() noexcept { resets_.fetch_add(1, std::memory_order_release); }
        // Any thread: the next warmupBlocks blocks count as startup.
        void warmUp() noexcept { warmups_.fetch_add(1, std::memory_order_relaxed); }

    private:
        // Written by the recording thread only, so plain loads and stores, no read-modify-write.
        struct Live
        {
            std::array<std::atomic<uint64_t>, Histogram::buckets> counts{};
            std::atomic<int64_t> maxNs{0}, maxAt{0};
            void add(int64_t ns, int64_t at) noexcept
            {
                auto& c = counts[Histogram::bucket(ns)];
                c.store(c.load(std::memory_order_relaxed) + 1, std::memory_order_relaxed);
                if(ns > maxNs.load(std::memory_order_relaxed)) { maxNs.store(ns, std::memory_order_relaxed); maxAt.store(at, std::memory_order_relaxed); }
            }
            void clear() noexcept { for(auto& c : counts) c.store(0, std::memory_order_relaxed); maxNs.store(0, std::memory_order_relaxed); maxAt.store(0, std::memory_order_relaxed); }
            Histogram read() const noexcept
            {
                Histogram h;
                for(size_t i = 0; i < h.buckets; ++i) h.counts[i] = counts[i].load(std::memory_order_relaxed);
                h.maxNs = maxNs.load(std::memory_order_relaxed); h.maxAt = maxAt.load(std::memory_order_relaxed);
                return h;
            }
        };
        Live steady_, startup_;
        std::atomic<uint32_t> resets_{0}, resetDone_{0}, warmups_{0};
        std::atomic<int64_t> last_{0};
        // The recording thread's own. A graph helper may record a node's block where the audio thread
        // recorded the last: the graph's join at each block's end orders them.
        uint32_t warmupSeen_ = 0, warmupLeft_ = warmupBlocks;
        int64_t origin_ = 0;
        bool started_ = false;
    };

    // A plugin worker's blocks as the engine takes them (RFC 0045's timing report), each measurement
    // a BlockTiming recorded by the thread that takes the block's result. roundTrip: from handing the
    // block over until the engine found it done or gave up waiting. collect: from the hand-over until
    // the engine's thread came for the result (the round trip less the engine's wait), long when that
    // thread was busy elsewhere or not running. wake: from the hand-over until the worker's audio thread
    // ran; run: from then until the worker was done with the block; both as the worker stamped them,
    // recorded once the engine sees the block done, if only as it hands over the next.
    struct WorkerTiming
    {
        BlockTiming roundTrip, collect, wake, run;
        void reset() noexcept { roundTrip.reset(); collect.reset(); wake.reset(); run.reset(); }
        void warmUp() noexcept { roundTrip.warmUp(); collect.warmUp(); wake.warmUp(); run.warmUp(); }
    };
}
