// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0004 step 1 through a plugin: the hardware fixture renders on the simulated accelerator when
// the profile declares it and on its CPU path otherwise, with the same output; two plugins cannot
// both hold its exclusive region; a failure while playing falls back to the CPU path without a gap
// when CPU time was reserved for it, and is reported, silent, when not; a late job is rendered on
// the CPU; offline rendering uses the CPU path; the host releases what an unloaded plugin held.
//   oscairn-hardware-plugin-test oscairn-hardware-fixture.clap simulated-profile.json
#include "simulated.h"
#include <kick_clap/Discovery.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <fstream>
#include <sstream>

using namespace oscairn::hardware;

namespace {
int failures = 0;
void expect(bool ok, const std::string& what) {
    std::printf("%s: %s\n", ok ? "ok" : "FAIL", what.c_str());
    if (!ok) ++failures;
}
constexpr uint32_t blocks = 400, frames = 128;
kick::clap::Descriptor fixture;

// Renders the fixture for blocks x frames, left channel; right must equal it.
std::vector<float> render(kick::clap::Host& host) {
    std::vector<float> out(blocks * frames), right(frames);
    for (uint32_t b = 0; b < blocks; ++b) {
        float* channels[] = {out.data() + b * frames, right.data()};
        if (!host.process(nullptr, 0, channels, 2, frames)) { std::printf("process failed: %s\n", host.error().c_str()); return {}; }
        if (!std::equal(right.begin(), right.end(), channels[0])) { std::printf("the channels differ\n"); return {}; }
    }
    return out;
}
std::vector<float> render(bool offline = false) {
    kick::clap::Host host;
    host.setOfflineRendering(offline);
    if (!host.load(fixture, 48000, frames)) { std::printf("load failed: %s\n", host.error().c_str()); return {}; }
    return render(host);
}
double difference(const std::vector<float>& a, const std::vector<float>& b, size_t from = 0, size_t to = SIZE_MAX) {
    if (a.size() != b.size() || a.empty()) return INFINITY;
    double worst = 0;
    for (size_t i = from; i < std::min(to, a.size()); ++i) worst = std::max(worst, double(std::fabs(a[i] - b[i])));
    return worst;
}
// The longest run of exact zeros: a dropout shows as one (a sine at 440 Hz never has two in a row).
size_t silence(const std::vector<float>& a, size_t from = 0) {
    size_t run = 0, longest = 0;
    for (size_t i = from; i < a.size(); ++i) { run = a[i] == 0 ? run + 1 : 0; longest = std::max(longest, run); }
    return longest;
}
std::vector<std::unique_ptr<Driver>> with(test::SimulatedDriver*& driver) {
    std::vector<std::unique_ptr<Driver>> d;
    auto sim = std::make_unique<test::SimulatedDriver>();
    driver = sim.get();
    d.push_back(std::move(sim));
    return d;
}
// The tolerance between the accelerator and the CPU path (RFC 0004: equivalent backends match within a stated tolerance).
constexpr double tolerance = 1e-6;
} // namespace

int main(int argc, char** argv) {
    if (argc != 3) { std::fprintf(stderr, "usage: oscairn-hardware-plugin-test fixture.clap simulated-profile.json\n"); return 2; }
    const auto scan = kick::clap::inspect(argv[1]);
    if (scan.plugins.empty()) { std::fprintf(stderr, "fixture not found\n"); return 1; }
    fixture = scan.plugins.front();
    std::ifstream f(argv[2]); std::stringstream text; text << f.rdbuf();
    const auto profile = fromProfile(text.str());

    // The CPU path: no service at all, then the CPU-only service.
    const auto reference = render();
    expect(reference.size() == blocks * frames && silence(reference) < 2, "the CPU path renders a sine (no service)");
    Service cpu(cpuOnly(), {});
    setCurrent(&cpu);
    expect(difference(render(), reference) == 0, "the CPU-only service: the same output, bit for bit");

    // The profile declares the simulated accelerator and the build has its service.
    {
        test::SimulatedDriver* sim = nullptr;
        Service s(profile, with(sim), 100);
        setCurrent(&s);
        kick::clap::Host host;
        expect(host.load(fixture, 48000, frames), "the fixture loads with the simulated accelerator");
        expect(s.json().find(R"("reserved":{"voices":1,"parts":0,"memory":4096,"regions":["osc"]})") != std::string::npos &&
               s.json().find(R"("cpuFallback":{"micros":100,"reserved":20})") != std::string::npos,
               "it reserved a voice, 4 KiB, the region and 20 us of CPU fallback when it activated");
        const auto out = render(host);
        const double d = difference(out, reference);
        std::printf("accelerator vs CPU path: %llu jobs, largest difference %.3g\n", (unsigned long long)sim->jobs.load(), d);
        expect(sim->jobs.load() == blocks, "every block rendered on the accelerator (" + std::to_string(sim->jobs.load()) + " jobs)");
        expect(d <= tolerance, "the same output as the CPU path, within " + std::to_string(tolerance));
        // A second instance wants the same exclusive region: refused, it plays on its CPU path.
        kick::clap::Host second;
        expect(second.load(fixture, 48000, frames), "a second instance loads");
        const auto jobs = sim->jobs.load();
        const auto out2 = render(second);
        expect(sim->jobs.load() == jobs && difference(out2, reference) == 0, "the second, refused the busy region, renders on its CPU path, identically");
        host.unload();
        expect(s.json().find(R"("reserved":{"voices":0,"parts":0,"memory":0,"regions":[]})") != std::string::npos,
               "unloading the plugin released its lease");
        setCurrent(nullptr);
    }
    // Declared in the profile, but this build has no service for it: the CPU path.
    {
        Service s(profile, {});
        setCurrent(&s);
        expect(difference(render(), reference) == 0 && s.json().find("this build has no service for oscairn.sim/1") != std::string::npos,
               "without the accelerator's service in the build, the CPU path, and the reason in the status");
        setCurrent(nullptr);
    }
    // The accelerator fails at block 200, with CPU time reserved for the fallback.
    {
        test::SimulatedDriver* sim = nullptr;
        Service s(profile, with(sim), 100);
        sim->failFrom = int64_t(200 * frames);
        setCurrent(&s);
        const auto out = render();
        const auto failed = s.failures();
        std::printf("failure at block 200 with a fallback: largest difference %.3g, longest silence %zu samples, %zu failure(s) reported\n",
                    difference(out, reference), silence(out), failed.size());
        expect(difference(out, reference) <= tolerance && silence(out) < 2, "the CPU path takes over in the failing block: no dropout");
        expect(failed.size() == 1 && failed[0].accelerator == "sim0" && failed[0].time == int64_t(200 * frames) && failed[0].cpuFallback,
               "the failure is reported: accelerator, time and the fallback");
        expect(s.unavailable(0) == "failed" && sim->jobs.load() == 201, "the accelerator is marked failed; no job was sent after it");
        setCurrent(nullptr);
    }
    // The same failure without CPU time for the fallback: the interruption is reported, and the plugin mutes.
    {
        test::SimulatedDriver* sim = nullptr;
        Service s(profile, with(sim), 0);
        sim->failFrom = int64_t(200 * frames);
        setCurrent(&s);
        const auto out = render();
        const auto failed = s.failures();
        const double before = difference(out, reference, 0, 200 * frames);
        size_t sounding = 0;
        for (size_t i = 200 * frames; i < out.size(); ++i) sounding += out[i] != 0;
        std::printf("failure at block 200 without a fallback: difference before %.3g, %zu nonzero samples after\n", before, sounding);
        expect(before <= tolerance && sounding == 0, "the accelerator's output until the failure, silence after it");
        expect(failed.size() == 1 && !failed[0].cpuFallback && s.json().find(R"("cpuFallback":false)") != std::string::npos,
               "the interruption is reported: no fallback was prepared");
        setCurrent(nullptr);
    }
    // Two late jobs: those blocks on the CPU path, the accelerator for the rest.
    {
        test::SimulatedDriver* sim = nullptr;
        Service s(profile, with(sim), 100);
        sim->lateFrom = int64_t(50 * frames);
        sim->lateUntil = int64_t(52 * frames);
        setCurrent(&s);
        const auto out = render();
        expect(difference(out, reference) <= tolerance && silence(out) < 2 && s.failures().empty() && s.unavailable(0).empty(),
               "late jobs are discarded and rendered on the CPU: no gap, no failure");
        setCurrent(nullptr);
    }
    // Offline rendering: the CPU reference path, even with the accelerator declared and offline-capable.
    {
        test::SimulatedDriver* sim = nullptr;
        Service s(profile, with(sim), 100);
        setCurrent(&s);
        const auto out = render(true);
        expect(out.size() == blocks * frames && difference(out, reference) == 0 && sim->jobs.load() == 0,
               "offline, the plugin renders on its CPU path: identical, no jobs");
        setCurrent(nullptr);
    }
    std::printf("%s\n", failures ? "FAIL" : "PASS");
    return failures ? 1 : 0;
}
