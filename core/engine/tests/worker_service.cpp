// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045: a worker that has had nothing to say is serviced less often. The engine's main thread asks each worker to
// service its plugin at most every workerServiceInterval (20 ms on a device), each a request, an answer and two
// switches; 22 workers at rest cost it most of a core. A worker that answered nothing for 25 services in a row is
// asked every fifth interval, until it says something or is asked anything else, when it is asked as often as before.
//   oscairn-worker-service-test WORKER NORMAL.clap
#include <oscairn/plugin_host.h>
#include <chrono>
#include <cstdio>
#include <stdexcept>
#include <string>
#include <thread>

using namespace std::chrono_literals;
static void check(bool ok, const std::string& why) { if(!ok) throw std::runtime_error(why); }

int main(int argc, char** argv)
try
{
    if(argc != 3) return 2;
    oscairn::PlacementPolicy policy; policy.worker = argv[1]; oscairn::setPlacementPolicy(policy);
    oscairn::setWorkerServiceInterval(20ms);
    auto host = oscairn::loadPlugin(argv[2], "org.kick.test.effect", 48000, 128);
    // Serviced as the engine's main thread does, a tick every 5 ms; the requests made in each span.
    auto ticks = [&](std::chrono::milliseconds span) {
        const auto before = oscairn::workerRequests();
        for(const auto end = std::chrono::steady_clock::now() + span; std::chrono::steady_clock::now() < end; std::this_thread::sleep_for(5ms)) host->service();
        return oscairn::workerRequests() - before;
    };
    const auto first = ticks(400ms);  // 20 services at the interval
    ticks(400ms);                     // 25 answers in all: quiet from here
    const auto quiet = ticks(1000ms); // one every 100 ms
    std::printf("services: %llu in the first 400 ms, %llu in a second of quiet\n", (unsigned long long)first, (unsigned long long)quiet);
    check(first >= 12, "serviced at the interval at first: " + std::to_string(first));
    check(quiet >= 5 && quiet <= 15, "a quiet worker asked every fifth interval: " + std::to_string(quiet) + " in 1 s");
    // Asked anything else, it is serviced as often as before.
    check(host->setParameter(host->parameters().front().id, host->parameters().front().minimum), "a parameter set");
    const auto busy = ticks(400ms);
    std::printf("services: %llu in the 400 ms after a parameter was set\n", (unsigned long long)busy);
    check(busy >= 12, "serviced at the interval again once asked something: " + std::to_string(busy));
    std::puts("ok: a quiet worker is serviced less often");
    return 0;
}
catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
