// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's plugin groups: a block's time in the engine by placement policy (compute.workers: plugin, chain, pool N),
// each plugin loaded under the group a Session would give it (placementGroups). Shapes: 22, an effect in the
// instrument's place fed by the input into five parts of three inserts, five sends every part feeds and a master; two,
// two instruments each through two inserts sharing a bus of two effects. Every effect is the fixture's, asleep on
// silent input (KICK_FIXTURE_EFFECT_SLEEP; KICK_FIXTURE_WORK_US gives each block work). Prints the block's p50 and p99
// at rest and with every chain fed, the first block after a second of silence, the worker processes and their
// resident memory together. The engine's thread real-time as a device's is; absolute times compare on one machine only.
//   oscairn-group-bench WORKER FIXTURE.clap plugin|chain|poolN 22|two [SECONDS]
#include <oscairn/graph.h>
#include <oscairn/realtime.h>
#include <oscairn/plugin_host.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <set>
#include <string>
#include <thread>
#include <vector>

int main(int argc, char** argv)
{
    if(argc < 5) { std::fprintf(stderr, "usage: %s WORKER FIXTURE.clap plugin|chain|poolN 22|two [SECONDS]\n", argv[0]); return 2; }
    setenv("KICK_FIXTURE_EFFECT_SLEEP", "1", 0);
    const std::string policyName = argv[3], shape = argv[4];
    const double seconds = argc > 5 ? std::atof(argv[5]) : 5;
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    oscairn::PlacementPolicy policy; policy.worker = argv[1];
    policy.chain = policyName == "chain";
    policy.hosts = policyName.rfind("pool", 0) == 0 ? uint32_t(std::max(1, std::atoi(policyName.c_str() + 4))) : 0;
    oscairn::setPlacementPolicy(policy);
    oscairn::Graph g{rate, frames, oscairn::ProcessingMode::Realtime, {}, oscairn::AudioLayout{{{0, "In", 2, 0}}, {{0, "Main", 2, 0}}}};

    std::vector<std::string> names; std::vector<std::pair<std::string, std::string>> audio; std::map<std::string, std::string> plugin;
    std::set<std::string> instruments;
    if(shape == "22")
    {
        names.push_back("inst");
        for(int p = 0; p < 5; ++p) for(int k = 0; k < 3; ++k) { const auto n = "p" + std::to_string(p) + "-" + std::to_string(k); names.push_back(n); audio.emplace_back(k ? "p" + std::to_string(p) + "-" + std::to_string(k - 1) : "inst", n); }
        for(int s = 0; s < 5; ++s) { names.push_back("s" + std::to_string(s)); for(int p = 0; p < 5; ++p) audio.emplace_back("p" + std::to_string(p) + "-2", "s" + std::to_string(s)); }
        names.push_back("master");
        for(int p = 0; p < 5; ++p) audio.emplace_back("p" + std::to_string(p) + "-2", "master");
        for(int s = 0; s < 5; ++s) audio.emplace_back("s" + std::to_string(s), "master");
    }
    else
    {
        names = {"i1", "a1", "b1", "i2", "a2", "b2", "bus1", "bus2", "master"};
        audio = {{"i1", "a1"}, {"a1", "b1"}, {"i2", "a2"}, {"a2", "b2"}, {"b1", "bus1"}, {"b2", "bus1"}, {"bus1", "bus2"}, {"b1", "master"}, {"b2", "master"}, {"bus2", "master"}};
        instruments = {"i1", "i2"};
    }
    const auto groups = oscairn::placementGroups(names, audio, "bench/");
    std::map<std::string, oscairn::NodeId> nodes;
    for(const auto& n : names)
    {
        const oscairn::PlacementScope scope(groups.at(n));
        nodes[n] = g.add(argv[2], instruments.count(n) ? "org.kick.test.midi" : "org.kick.test.effect");
        if(instruments.count(n)) g.connectNotes(oscairn::graphInput, nodes[n]);
    }
    if(shape == "22") g.connectAudio(oscairn::AudioConnection{{oscairn::graphInput, 0, 0}, {nodes["inst"], 0, 0}, 2});
    for(const auto& [a, b] : audio) g.connectAudio(oscairn::AudioConnection{{nodes[a], 0, 0}, {nodes[b], 0, 0}, 2, b[0] == 's' && a[0] == 'p' ? .2 : 1});
    g.connectAudio(oscairn::AudioConnection{{nodes["master"], 0, 0}, {oscairn::graphOutput, 0, 0}, 2});
    g.commit();
    uint32_t grouped = 0; std::set<const void*> lanes;
    for(const auto& [n, id] : nodes) { grouped += g.grouped(id); lanes.insert(g.host(id).lane()); }

    std::vector<float> buffer(2 * frames), input(2 * frames);
    float* out[2]{buffer.data(), buffer.data() + frames}; const float* in[2]{input.data(), input.data() + frames};
    const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
    const bool realtime = oscairn::worker::makeRealtime(1e6 * frames / rate, oscairn::worker::priority::engineAudio);
    bool held = false;
    auto block = [&](bool sound) {
        for(uint32_t i = 0; i < frames; ++i) input[i] = input[frames + i] = sound ? .1f : 0.f;
        std::vector<kick::clap::Event> events;
        if(!instruments.empty() && sound != held) { events.push_back(kick::clap::Event::note(sound, 60, .8)); held = sound; }
        const auto t = std::chrono::steady_clock::now();
        oscairn::worker::callbackDeadline = t + std::chrono::nanoseconds(int64_t(double(period.count()) * (1 - oscairn::worker::mixingReserve)));
        g.process(in, 2, out, 2, frames, nullptr, events.data(), uint32_t(events.size()));
        oscairn::worker::callbackDeadline = {};
        return std::chrono::duration<double, std::micro>(std::chrono::steady_clock::now() - t).count();
    };
    auto next = std::chrono::steady_clock::now();
    auto paced = [&](bool sound) { std::this_thread::sleep_until(next += period); return block(sound); };
    for(int i = 0; i < 800; ++i) paced(false); // the workers started, past the hold
    const auto blocks = uint32_t(seconds * rate / frames);
    std::vector<double> rest, fed, first;
    for(uint32_t i = 0; i < blocks; ++i) rest.push_back(paced(false));
    for(uint32_t i = 0; i < blocks; ++i) fed.push_back(paced(true));
    for(int take = 0; take < 12; ++take)
    {
        for(int i = 0; i < 375; ++i) paced(false); // a second of silence
        first.push_back(paced(true));
        for(int i = 0; i < 20; ++i) paced(true);
    }
    auto pct = [](std::vector<double> v, double p) { std::sort(v.begin(), v.end()); return v[std::min(v.size() - 1, size_t(p * double(v.size() - 1)))]; };
    uint64_t resident = 0; size_t processes = 0;
    for(const auto& h : oscairn::pluginHosts()) if(h.pid > 0 && !h.plugins.empty()) { ++processes; resident += oscairn::residentBytes(h.pid); }
    uint64_t missed = 0; for(const auto& [n, id] : nodes) { const auto s = g.host(id).statistics(); missed += s.lateBlocks + s.silentBlocks; }
    std::printf("%s %s: %zu processes (%zu lanes), %u of %zu in their sources' round trip, %.1f MiB resident; block us at rest p50 %.0f p99 %.0f, fed p50 %.0f p99 %.0f; "
                "first after silence p50 %.0f max %.0f; missed %llu%s\n",
                shape.c_str(), policyName.c_str(), processes, lanes.size(), grouped, nodes.size(), double(resident) / (1 << 20), pct(rest, .5), pct(rest, .99), pct(fed, .5), pct(fed, .99),
                pct(first, .5), *std::max_element(first.begin(), first.end()), static_cast<unsigned long long>(missed), realtime ? "" : " (not real-time)");
}
