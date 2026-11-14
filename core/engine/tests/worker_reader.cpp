// SPDX-License-Identifier: AGPL-3.0-or-later
// A worker's reply carries a record count; one the rest of the reply cannot hold is refused before
// anything is sized by it (Reader::count).
#include "Worker.h"
#include <cstdio>

int main()
{
    using namespace oscairn::worker;
    int failures = 0;
    const auto refused = [](uint32_t n, size_t records)
    {
        Reader r{Writer().u32(n).data + std::string(records * 16, 'x')};
        try { r.count(16); return false; } catch(const std::runtime_error&) { return true; }
    };
    if(!refused(UINT32_MAX, 0)) { std::fprintf(stderr, "FAIL: the largest count was accepted\n"); ++failures; }
    if(!refused(3, 2)) { std::fprintf(stderr, "FAIL: a count past the reply's end was accepted\n"); ++failures; }
    if(refused(2, 2)) { std::fprintf(stderr, "FAIL: a count the reply holds was refused\n"); ++failures; }
    return failures ? 1 : 0;
}
