// SPDX-License-Identifier: AGPL-3.0-or-later
// The engine thread's cost per block for a session shaped like the FANTOM-07's heavy take (notes/w27/cost-2026-11-21.md):
// chain placement, four piano parts (the voice fixture giving up its own voices and reporting them, the sustain pedal
// taken) each through an insert, with a bus placed beside the insert by the document's wire but fed by nothing (the part's
// send at 0; with `send0` wired at gain 0, as a send at 0 was compiled before 7fa528f7), and a lead through an insert. At
// rest, then each piano holding NOTES keys and the lead two (keys held, not the pedal: the fixture got no pedal before RFC
// 0046's fixes, so held keys compare across commits), on a real-time thread paced at the period with the load guard on.
// Prints the engine thread's CPU time per block (the figure the unit's profile reads), the block's wall time, the worker
// processes and the blocks lost. Absolute times compare on one machine only.
//   oscairn-session-bench WORKER VOICE_FIXTURE.clap [NOTES] [SECONDS] [send0]
#include <oscairn/graph.h>
#include <oscairn/realtime.h>
#include <oscairn/plugin_host.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <map>
#include <set>
#include <string>
#include <thread>
#include <vector>

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: %s WORKER VOICE_FIXTURE.clap [NOTES] [SECONDS] [send0]\n", argv[0]); return 2; }
    const int notes = argc > 3 ? std::atoi(argv[3]) : 60;
    const double seconds = argc > 4 ? std::atof(argv[4]) : 10;
    const bool send0 = argc > 5 && std::string(argv[5]) == "send0";
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    oscairn::PlacementPolicy policy; policy.worker = argv[1]; policy.chain = true;
    oscairn::setPlacementPolicy(policy);
    setenv("OSCAIRN_FIXTURE_VOICE_US", "2", 1); setenv("OSCAIRN_FIXTURE_DECAY_S", "30", 1);
    setenv("OSCAIRN_FIXTURE_EFFECT_US", "5", 1);
    oscairn::Graph g{rate, frames};
    std::vector<std::string> names; std::vector<std::pair<std::string, std::string>> wires;
    for(int p = 0; p < 4; ++p)
    {
        const auto n = std::to_string(p);
        for(const auto& x : {"piano" + n, "insert" + n, "bus" + n}) names.push_back(x);
        wires.push_back({"piano" + n, "insert" + n}); wires.push_back({"insert" + n, "bus" + n});
    }
    names.push_back("lead"); names.push_back("leadInsert"); wires.push_back({"lead", "leadInsert"});
    const auto groups = oscairn::placementGroups(names, wires, "session/");
    std::map<std::string, oscairn::NodeId> nodes;
    for(const auto& n : names)
    {
        const bool instrument = n.rfind("piano", 0) == 0 || n == "lead";
        if(instrument) setenv("OSCAIRN_FIXTURE_VOICE_BUDGET", "1", 1); else unsetenv("OSCAIRN_FIXTURE_VOICE_BUDGET");
        const oscairn::PlacementScope scope(groups.at(n));
        nodes[n] = instrument ? g.add(argv[2]) : g.add(argv[2], "org.oscairn.test.costly-effect");
    }
    for(int p = 0; p < 5; ++p)
    {
        oscairn::NoteZone z; z.channels = uint16_t(1u << p); z.part = uint32_t(p + 1); z.priority = uint8_t(p == 4 ? 10 : 8);
        g.connectNotes(oscairn::graphInput, nodes[p < 4 ? "piano" + std::to_string(p) : "lead"], {z});
    }
    for(int p = 0; p < 4; ++p)
    {
        const auto n = std::to_string(p);
        g.connectAudio(nodes["piano" + n], nodes["insert" + n]); g.connectAudio(nodes["insert" + n], oscairn::graphOutput);
        g.connectAudio(nodes["bus" + n], oscairn::graphOutput);
        if(send0) g.connectAudio(oscairn::AudioConnection{{nodes["insert" + n], 0, 0}, {nodes["bus" + n], 0, 0}, 2, 0.0});
    }
    g.connectAudio(nodes["lead"], nodes["leadInsert"]); g.connectAudio(nodes["leadInsert"], oscairn::graphOutput);
    g.commit();
    g.setVoiceBudget(-1);
    std::set<const void*> lanes; uint32_t grouped = 0;
    for(const auto& [n, id] : nodes) { lanes.insert(g.host(id).lane()); grouped += g.grouped(id); }

    struct Phase { double cpu = 0; std::vector<double> walls, cpus; uint32_t blocks = 0; };
    Phase rest, held;
    std::thread([&] {
        oscairn::worker::makeRealtime(frames / rate * 1e6, oscairn::worker::priority::engineAudio);
        std::vector<float> left(frames), right(frames); float* out[]{left.data(), right.data()};
        const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
        auto next = std::chrono::steady_clock::now();
        auto cpu = [] { timespec t{}; clock_gettime(CLOCK_THREAD_CPUTIME_ID, &t); return double(t.tv_sec) * 1e6 + double(t.tv_nsec) / 1e3; };
        auto block = [&](const std::vector<kick::clap::Event>& e, Phase* phase) {
            const auto t = std::chrono::steady_clock::now(); const double c = cpu();
            oscairn::worker::callbackDeadline = t + std::chrono::nanoseconds(int64_t(double(period.count()) * (1 - oscairn::worker::mixingReserve)));
            g.process(out, frames, nullptr, e.data(), uint32_t(e.size()));
            oscairn::worker::callbackDeadline = {};
            if(phase)
            {
                const double used = cpu() - c; phase->cpu += used; phase->cpus.push_back(used); ++phase->blocks;
                phase->walls.push_back(std::chrono::duration<double, std::micro>(std::chrono::steady_clock::now() - t).count());
            }
            std::this_thread::sleep_until(next += period);
        };
        const auto blocks = uint32_t(seconds * rate / frames);
        for(int i = 0; i < 800; ++i) block({}, nullptr); // the workers started, past the hold
        for(uint32_t i = 0; i < blocks; ++i) block({}, &rest);
        std::vector<kick::clap::Event> e;
        for(int k = 0; k < notes; ++k)
        {
            e.clear();
            for(int p = 0; p < 4; ++p) e.push_back(kick::clap::Event::note(true, 30 + k, .8, 0, int16_t(p)));
            if(k < 2) e.push_back(kick::clap::Event::note(true, 80 + k, .8, 0, 4));
            block(e, nullptr);
        }
        for(int i = 0; i < 200; ++i) block({}, nullptr);
        for(uint32_t i = 0; i < blocks; ++i) block({}, &held);
    }).join();
    auto pct = [](std::vector<double> v, double p) { std::sort(v.begin(), v.end()); return v.empty() ? 0 : v[std::min(v.size() - 1, size_t(p * double(v.size() - 1)))]; };
    uint64_t missed = 0, gave = 0; uint32_t sounding = 0;
    for(const auto& [n, id] : nodes)
    {
        const auto s = g.host(id).statistics(); missed += s.lateBlocks + s.silentBlocks;
        const auto c = g.noteCost(id); gave += c.stolen + c.stolenForDevice + c.hardChokes + c.stolenForLoss + c.stolenForDeadline;
        sounding += g.sounding(id);
    }
    size_t processes = 0; for(const auto& h : oscairn::pluginHosts()) if(h.pid > 0 && !h.plugins.empty()) ++processes;
    std::printf("%zu processes, %zu lanes, %u nodes in a group's round trip; engine thread CPU us/block mean (p50): rest %.1f (%.1f), %d notes x4 + lead %.1f (%.1f); "
                "block wall p50/p99 us: rest %.0f/%.0f, held %.0f/%.0f; notes held at the end %u, given up %llu, blocks missed %llu\n",
                processes, lanes.size(), grouped, rest.cpu / rest.blocks, pct(rest.cpus, .5), notes, held.cpu / held.blocks, pct(held.cpus, .5), pct(rest.walls, .5), pct(rest.walls, .99),
                pct(held.walls, .5), pct(held.walls, .99), sounding, static_cast<unsigned long long>(gave), static_cast<unsigned long long>(missed));
}
