// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045: a worker that has had nothing to say is serviced less often. The engine's main thread asks each worker to
// service its plugin at most every workerServiceInterval (20 ms on a device), each a request, an answer and two
// switches; 22 workers at rest cost it most of a core. A worker that answered nothing for 25 services in a row is
// asked every fifth interval, until it says something or is asked anything else, when it is asked as often as before.
// Each tick is checked against that rule by the clock it ran at, so a slow machine (ticks late, fewer of them) changes
// how many services there are, never whether each one was due.
//   oscairn-worker-service-test WORKER NORMAL.clap
#include <oscairn/plugin_host.h>
#include "../../test-support/relaxed.h"
#include <chrono>
#include <cstdio>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

using namespace std::chrono_literals;
static void check(bool ok, const std::string& why) { if(!ok) throw std::runtime_error(why); }

int main(int argc, char** argv)
try
{
    if(argc != 3) return 2;
    using clock = std::chrono::steady_clock;
    constexpr auto interval = 20ms;
    constexpr uint32_t quietAfter = 25, factor = 5;
    oscairn::PlacementPolicy policy; policy.worker = argv[1]; oscairn::setPlacementPolicy(policy);
    oscairn::setWorkerServiceInterval(interval);
    auto host = oscairn::loadPlugin(argv[2], "org.kick.test.effect", 48000, 128);
    // Serviced as the engine's main thread does, a tick every 5 ms. A tick serviced the worker if it made a request. The
    // last service's tick bounds when it was asked: after the tick's start, before its end.
    clock::time_point lastStart{}, lastEnd{};
    bool any = false;
    uint32_t answers = 0; // services since the worker was last asked anything else, each answered with nothing
    struct Seen { uint32_t services = 0, atInterval = 0, heldBack = 0; }; // services, of them due only at the interval, ticks held back past it
    auto ticks = [&](std::chrono::milliseconds span) {
        Seen seen;
        for(const auto end = clock::now() + span; clock::now() < end; std::this_thread::sleep_for(5ms))
        {
            const auto before = oscairn::workerRequests();
            const auto start = clock::now();
            host->service();
            const auto done = clock::now();
            const auto made = oscairn::workerRequests() - before;
            check(made <= 1, "a tick makes at most one request: " + std::to_string(made));
            const auto due = answers >= quietAfter ? factor * interval : interval;
            if(made)
            {
                // Asked no sooner than due after the last service: the most that can have passed between the two asks.
                check(!any || done - lastStart >= due, "a service only once due: " + std::to_string(std::chrono::duration_cast<std::chrono::microseconds>(done - lastStart).count()) +
                      " us after the last, due after " + std::to_string(std::chrono::duration_cast<std::chrono::milliseconds>(due).count()) + " ms, " + std::to_string(answers) + " quiet answers");
                if(any && done - lastStart < factor * interval) ++seen.atInterval;
                any = true; lastStart = start; lastEnd = done; ++answers; ++seen.services;
            }
            else
            {
                // Not asked though due: the least that can have passed between the last ask and this tick's.
                check(any && start - lastEnd < due, "serviced once due: " + std::to_string(std::chrono::duration_cast<std::chrono::microseconds>(start - lastEnd).count()) +
                      " us after the last, due after " + std::to_string(std::chrono::duration_cast<std::chrono::milliseconds>(due).count()) + " ms, " + std::to_string(answers) + " quiet answers");
                if(start - lastEnd >= interval) ++seen.heldBack;
            }
        }
        return seen;
    };
    const auto first = ticks(400ms);  // 20 services at the interval
    ticks(400ms);                     // 25 answers in all: quiet from here
    const auto quiet = ticks(1000ms); // one every 100 ms
    std::printf("services: %u in the first 400 ms (%u sooner than a quiet worker's), %u in a second of quiet (%u ticks held back past the interval)\n",
                first.services, first.atInterval, quiet.services, quiet.heldBack);
    // Shown, not only allowed: services at the interval at first, then ticks past it held back. A tick comes every 5 ms, so a
    // run without either had ticks late by more than the quiet interval all along: only a stalled machine (relaxed) says so.
    const bool relaxed = oscairn::test::relaxed();
    if(relaxed && (!first.atInterval || !quiet.heldBack)) std::puts("not shown: the ticks came too late to show the interval or the quiet one");
    else
    {
        check(first.atInterval > 0, "serviced at the interval at first");
        check(quiet.heldBack > 0, "a quiet worker held back past the interval");
    }
    // Asked anything else, it is serviced as often as before.
    check(host->setParameter(host->parameters().front().id, host->parameters().front().minimum), "a parameter set");
    answers = 0;
    const auto busy = ticks(400ms);
    std::printf("services: %u in the 400 ms after a parameter was set (%u sooner than a quiet worker's)\n", busy.services, busy.atInterval);
    if(!(relaxed && !busy.atInterval)) check(busy.atInterval > 0, "serviced at the interval again once asked something");
    std::puts("ok: a quiet worker is serviced less often");
    return 0;
}
catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
