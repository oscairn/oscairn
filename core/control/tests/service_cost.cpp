// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A measurement, not a test: what the control thread does at rest. A Performance of 25 plugins in workers (five parts
// of an instrument, one instance, and four effects each), every tick of the engine's main thread (Session::service,
// every 5 ms) measured in thread CPU time, as the unit's "Sick tines" held it at 73 to 89 percent of a core. Run with
// OSCAIRN_PLUGIN_WORKER, OSCAIRN_PLUGIN_SCAN and OSCAIRN_WORKER_SERVICE_MS=20 as a device sets them; STUCK=1 leaves
// a value pending that a shared node refuses for good; TICKS the number of ticks (400); BUDGET=1 measures under a budget.
// Any parameter count: the control fixture (oscairn-control-fixture) as both plugins with OSCAIRN_CONTROL_FIXTURE_PARAMETERS=N
// gives 25 plugins of N parameters each (an instrument that cannot share its instance takes one a part).
//   oscairn-service-cost INSTRUMENT.clap EFFECT.clap
#include "oscairn/session.h"
#include "oscairn/plugin_host.h"
#include "../../plugin-host/src/StateFiles.h"
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <string>
#include <thread>
#include <vector>
#include <array>

using namespace oscairn;

namespace
{
    double threadSeconds() { timespec t{}; clock_gettime(CLOCK_THREAD_CPUTIME_ID, &t); return double(t.tv_sec) + 1e-9 * double(t.tv_nsec); }
}

int main(int argc, char** argv)
{
    if(argc != 3) { std::fprintf(stderr, "usage: %s INSTRUMENT.clap EFFECT.clap\n", argv[0]); return 2; }
    state_files::TemporaryDirectory home;
    ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
    Session s(48000, 128);
    if(std::getenv("BUDGET")) s.setBudget({.9, 0}); // a device's: the budget's measuring runs every tick too
    size_t parameters = 0;
    NodeId first = 0;
    std::vector<NodeId> nodes;
    for(uint32_t p = 0; p < 5; ++p)
    {
        const auto name = "instrument " + std::to_string(p);
        auto instrument = [&] {
            if(!p) return first = s.add(name, argv[1]);
            try { return s.shareInstrument(name, first); } catch(const std::runtime_error&) { return s.add(name, argv[1]); }
        };
        const auto n = instrument(); // one instance, five sounds, as Sick tines
        PartInput keys; keys.zones = {NoteZone{uint8_t(24 * p), uint8_t(24 * p + 23)}};
        const auto part = s.addPart("Part " + std::to_string(p), keys);
        s.insertIntoPart(part, 0, n); nodes.push_back(n);
        parameters += s.parameters(n).size();
        for(uint32_t i = 0; i < 4; ++i) { const auto e = s.add("effect " + std::to_string(p) + "-" + std::to_string(i), argv[2]); s.insertIntoPart(part, 1 + i, e); parameters += s.parameters(e).size(); }
        s.commit();
    }
    if(std::getenv("STUCK")) for(const auto n : nodes) for(const auto& q : s.parameters(n)) if(q.role == OSCAIRN_ROLE_OUTPUT_BUS) s.setParameter(n, q.id, s.parameter(n, q.id) == 3 ? 4 : 3); // a value a shared node refuses for good, as a scene stored with another bus asks
    std::array<float, 128> l{}, r{};
    float* out[2]{l.data(), r.data()};
    for(int i = 0; i < 100; ++i) { s.service(); s.process(nullptr, 0, out, 2, 128); }
    if(std::getenv("BLOCKS")) // the blocks' cost too, the engine thread's CPU and the wall time of each
    {
        const int n = std::atoi(std::getenv("BLOCKS"));
        for(int i = 0; i < 200; ++i) s.process(nullptr, 0, out, 2, 128);
        const double c0 = threadSeconds(); const auto w0 = std::chrono::steady_clock::now();
        for(int i = 0; i < n; ++i) s.process(nullptr, 0, out, 2, 128);
        const double c1 = threadSeconds(); const auto w1 = std::chrono::steady_clock::now();
        std::printf("process: %.2f us CPU, %.2f us wall a block\n", 1e6 * (c1 - c0) / n, std::chrono::duration<double, std::micro>(w1 - w0).count() / n);
    }
    const int ticks = std::getenv("TICKS") ? std::atoi(std::getenv("TICKS")) : 400;
    const auto requests = workerRequests();
    const auto compared = s.parametersCompared();
    const double before = threadSeconds();
    for(int i = 0; i < ticks; ++i) { s.service(); std::this_thread::sleep_for(std::chrono::milliseconds(5)); }
    const double cpu = threadSeconds() - before;
    std::printf("%zu parameters over 25 plugins: %.1f us of CPU a service tick, %.1f%% of a core at one tick every 5 ms; %.0f requests to workers a second\n", parameters, 1e6 * cpu / ticks, 100 * cpu / ticks / .005, double(workerRequests() - requests) / (ticks * .005));
    std::printf("%.0f parameter values compared a second\n", double(s.parametersCompared() - compared) / (ticks * .005));
    if(std::getenv("STUCK")) { try { s.savePerformance((home.path / "saved").string()); std::printf("a save is taken\n"); } catch(const std::exception& e) { std::printf("a save is refused: %s\n", e.what()); } }
    return 0;
}
