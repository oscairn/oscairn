// SPDX-License-Identifier: AGPL-3.0-or-later
// oscairn-stalling-worker: the plugin worker, but one block, whatever the plugin, taking long before
// the plugin renders it (worker-miss, for a plugin with no stall of its own, as a shared Basic Synth):
// OSCAIRN_TEST_WORKER_STALL=BLOCK:MICROSECONDS, BLOCK counted from 1 in this process.
#include "Worker.h"
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <thread>

namespace oscairn::worker
{
    void stall() noexcept
    {
        struct Stall { long block = 0, us = 0; };
        static const Stall stall = [] { Stall s; if(const char* v = std::getenv("OSCAIRN_TEST_WORKER_STALL")) std::sscanf(v, "%ld:%ld", &s.block, &s.us); return s; }();
        static long calls = 0;
        if(++calls == stall.block) std::this_thread::sleep_for(std::chrono::microseconds(stall.us));
    }
}
