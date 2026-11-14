// SPDX-License-Identifier: AGPL-3.0-or-later
// The engine's meter and quiet loops take the largest |sample| of a block with std::fmax, which a
// compiler vectorizes (std::max, a compare-and-select, stays scalar). Both give the same bits for every
// block, ordinary or holding a NaN or an infinity: the loop starts at 0 and std::max(peak, NaN) is peak.
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <limits>
#include <random>
#include <vector>

int main()
{
    std::mt19937 random(7);
    std::uniform_real_distribution<float> level(-2.f, 2.f);
    int failures = 0;
    for(int block = 0; block < 2000; ++block)
    {
        std::vector<float> x(size_t(1 + random() % 512));
        for(auto& v : x) v = level(random);
        if(block % 3 == 1) x[random() % x.size()] = std::numeric_limits<float>::quiet_NaN();
        if(block % 5 == 2) x[random() % x.size()] = (block & 1 ? 1 : -1) * std::numeric_limits<float>::infinity();
        if(block % 7 == 3) x[0] = std::numeric_limits<float>::quiet_NaN();
        float before = 0, after = 0;
        for(float v : x) before = std::max(before, std::abs(v));
        for(float v : x) after = std::fmax(after, std::abs(v));
        if(std::memcmp(&before, &after, sizeof before)) { std::fprintf(stderr, "FAIL: block %d: %g against %g\n", block, before, after); ++failures; }
    }
    return failures ? 1 : 0;
}
