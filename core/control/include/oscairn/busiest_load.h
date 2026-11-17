// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0025: a plugin's busiest sustained load (measurements.json's cpu), robust to a spike. Each service tick gives
// the plugin's mean share of the block over that tick's blocks; the busiest load follows a mean of about a second
// (fastBlocks) of the median of the last `window` ticks, so a stall, a burst of page faults or a block of work at a
// load, lasting fewer than half of those ticks, moves it not at all. Control thread.
#pragma once
#include <algorithm>
#include <array>
#include <cstdint>

namespace oscairn
{
    struct BusiestLoad
    {
        static constexpr size_t window = 5; // ticks: 100 ms at a device's 20 ms service, 25 ms at a desktop's 5 ms
        static constexpr double fastBlocks = 400; // about a second at 128 frames and 48 kHz
        std::array<double, window> recent{};
        uint64_t ticks = 0;
        double fast = 0;
        // A tick's share over its blocks; false until the window is full (nothing to say yet).
        bool add(double share, uint64_t blocks)
        {
            recent[ticks++ % window] = share;
            if(ticks < window) return false;
            auto sorted = recent;
            std::nth_element(sorted.begin(), sorted.begin() + window / 2, sorted.end());
            fast += (sorted[window / 2] - fast) * std::min(1.0, double(blocks) / fastBlocks); // from 0: one tick is not a second's load
            return true;
        }
    };
}
