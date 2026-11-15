// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's plugin groups: a block's time in the engine for a 22-plugin Performance's shape (Sick tines on the
// FANTOM-07: one instrument feeding five parts of three inserts each, five sends every part feeds, a master), every
// plugin the fixture's effect sleeping on silent input, at rest and on the first block after a second of silence, by
// placement: a worker per plugin (HOSTS 0) or that many plugin hosts, with groups on or off (OSCAIRN_WORKER_GROUPS).
// The engine's thread real-time as a device's is; absolute times only compare on one machine.
//   oscairn-group-bench WORKER FIXTURE.clap HOSTS [SECONDS]
#include <oscairn/graph.h>
#include <oscairn/realtime.h>
#include <oscairn/plugin_host.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>
#include <vector>

int main(int argc, char** argv)
{
    if(argc < 4) { std::fprintf(stderr, "usage: %s WORKER FIXTURE.clap HOSTS [SECONDS]\n", argv[0]); return 2; }
    setenv("KICK_FIXTURE_EFFECT_SLEEP", "1", 0);
    const uint32_t hosts = uint32_t(std::atoi(argv[3]));
    const double seconds = argc > 4 ? std::atof(argv[4]) : 10;
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    oscairn::PlacementPolicy policy; policy.worker = argv[1]; policy.hosts = hosts; oscairn::setPlacementPolicy(policy);
    oscairn::Graph g{rate, frames, oscairn::ProcessingMode::Realtime, {}, oscairn::AudioLayout{{{0, "In", 2, 0}}, {{0, "Main", 2, 0}}}};
    auto add = [&] { return g.add(argv[2], "org.kick.test.effect"); };
    auto wire = [&](oscairn::NodeId from, oscairn::NodeId to, double gain = 1) { g.connectAudio(oscairn::AudioConnection{{from, 0, 0}, {to, 0, 0}, 2, gain}); };
    const auto instrument = add();
    g.connectAudio(oscairn::AudioConnection{{oscairn::graphInput, 0, 0}, {instrument, 0, 0}, 2});
    std::vector<oscairn::NodeId> parts, sends;
    for(int p = 0; p < 5; ++p) { auto a = add(), b = add(), c = add(); wire(instrument, a); wire(a, b); wire(b, c); parts.push_back(c); }
    for(int s = 0; s < 5; ++s) { sends.push_back(add()); for(const auto p : parts) wire(p, sends.back(), .2); }
    const auto master = add();
    for(const auto p : parts) wire(p, master);
    for(const auto s : sends) wire(s, master, .5);
    g.connectAudio(oscairn::AudioConnection{{master, 0, 0}, {oscairn::graphOutput, 0, 0}, 2});
    g.commit();
    uint32_t grouped = 0; for(const auto& n : g.describe().nodes) grouped += g.grouped(n.id);

    std::vector<float> buffer(2 * frames), input(2 * frames);
    float* out[2]{buffer.data(), buffer.data() + frames}; const float* in[2]{input.data(), input.data() + frames};
    const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
    const bool realtime = oscairn::worker::makeRealtime(1e6 * frames / rate, 80);
    auto block = [&](bool sound) {
        for(uint32_t i = 0; i < frames; ++i) input[i] = input[frames + i] = sound ? .1f : 0.f;
        const auto t = std::chrono::steady_clock::now();
        oscairn::worker::callbackDeadline = t + std::chrono::nanoseconds(int64_t(double(period.count()) * (1 - oscairn::worker::mixingReserve)));
        g.process(in, 2, out, 2, frames, nullptr, nullptr, 0);
        oscairn::worker::callbackDeadline = {};
        return std::chrono::duration<double, std::micro>(std::chrono::steady_clock::now() - t).count();
    };
    auto next = std::chrono::steady_clock::now();
    auto paced = [&](bool sound) { std::this_thread::sleep_until(next += period); return block(sound); };
    for(int i = 0; i < 800; ++i) paced(false); // the workers started, past the hold
    std::vector<double> rest, first;
    const auto blocks = uint32_t(seconds * rate / frames);
    for(uint32_t i = 0; i < blocks; ++i) rest.push_back(paced(false));
    for(int take = 0; take < 12; ++take)
    {
        for(int i = 0; i < 375; ++i) paced(false); // a second of silence
        first.push_back(paced(true));
        for(int i = 0; i < 20; ++i) paced(true);
    }
    auto pct = [](std::vector<double> v, double p) { std::sort(v.begin(), v.end()); return v[std::min(v.size() - 1, size_t(p * double(v.size() - 1)))]; };
    uint64_t missed = 0; for(const auto& n : g.describe().nodes) { const auto s = g.host(n.id).statistics(); missed += s.lateBlocks + s.silentBlocks; }
    const char* groups = std::getenv("OSCAIRN_WORKER_GROUPS");
    std::printf("hosts %u groups %s: %u of 22 grouped; at rest block us p50 %.0f p90 %.0f p99 %.0f; first block after silence p50 %.0f max %.0f; missed %llu%s\n",
                hosts, groups && std::string(groups) == "0" ? "off" : "on", grouped, pct(rest, .5), pct(rest, .9), pct(rest, .99), pct(first, .5),
                *std::max_element(first.begin(), first.end()), static_cast<unsigned long long>(missed), realtime ? "" : " (not real-time)");
}
