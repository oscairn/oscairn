// SPDX-License-Identifier: AGPL-3.0-or-later
// engine-cost-budget: the engine side of the worker round trips, counted per block (WorkerTiming's handed, copied, wakes,
// sleeps, in this test's own build of the plugin host and engine, OSCAIRN_COST_COUNTERS) and held to recorded budgets (docs/developer-guide.md, "Per-block cost budgets"): blocks handed to workers,
// bytes copied between the engine and the shared blocks, and the system calls made for them, at rest and with keys held.
// Five instruments (the voice fixture, oscairn.voice-budget draft 2) each through an insert, chain placement, the budget
// on, and a sixth instrument kept warm (loaded, released from the graph, as Session keeps one, RFC 0021), which must cost
// the engine thread nothing. Counts, not times: a change that adds per-block work fails on any machine. The core's own
// work (entries, slots, ranking, calls) is engine-cost-budget-core's (core/engine/rust/src/run.rs, cost_budget).
//   oscairn-cost-budget-test WORKER VOICE_FIXTURE.clap
#include <oscairn/graph.h>
#include <oscairn/realtime.h>
#include <oscairn/plugin_host.h>
#include <oscairn/timing.h>
#include <array>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

namespace
{
struct Work { double handed = 0, copied = 0, calls = 0; };
Work operator-(Work a, const Work& b) { a.handed -= b.handed; a.copied -= b.copied; a.calls -= b.calls; return a; }
Work count(const std::vector<oscairn::PluginHost*>& hosts)
{
    Work w;
    for(auto* h : hosts)
        if(auto* t = h->workerTiming())
        {
            w.handed += double(t->handed.load()); w.copied += double(t->copied.load());
            w.calls += double(t->wakes.load() + t->sleeps.load());
        }
    return w;
}
}

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: %s WORKER VOICE_FIXTURE.clap\n", argv[0]); return 2; }
    constexpr int parts = 5; constexpr double rate = 48000; constexpr uint32_t frames = 128;
    oscairn::PlacementPolicy policy; policy.worker = argv[1]; policy.chain = true;
    oscairn::setPlacementPolicy(policy);
    setenv("OSCAIRN_AUDIO_HELPERS", "0", 1); setenv("OSCAIRN_LOAD_GUARD", "off", 1);
    setenv("OSCAIRN_FIXTURE_VOICE_US", "1", 1); setenv("OSCAIRN_FIXTURE_EFFECT_US", "1", 1);
    oscairn::Graph g{rate, frames};
    std::vector<std::string> names; std::vector<std::pair<std::string, std::string>> wires;
    for(int p = 0; p <= parts; ++p)
    {
        const auto n = std::to_string(p);
        names.push_back("synth" + n); names.push_back("insert" + n); wires.push_back({"synth" + n, "insert" + n});
    }
    const auto groups = oscairn::placementGroups(names, wires, "cost/");
    std::shared_ptr<oscairn::PluginHost> kept;
    std::vector<oscairn::PluginHost*> played;
    for(int p = 0; p <= parts; ++p)
    {
        const auto n = std::to_string(p);
        setenv("OSCAIRN_FIXTURE_VOICE_BUDGET", "1", 1);
        oscairn::NodeId synth, insert;
        { const oscairn::PlacementScope scope(groups.at("synth" + n)); synth = g.add(argv[2]); }
        unsetenv("OSCAIRN_FIXTURE_VOICE_BUDGET");
        { const oscairn::PlacementScope scope(groups.at("insert" + n)); insert = g.add(argv[2], "org.oscairn.test.costly-effect"); }
        oscairn::NoteZone z; z.channels = uint16_t(1u << p); z.part = uint32_t(p + 1);
        g.connectNotes(oscairn::graphInput, synth, {z});
        g.connectAudio(synth, insert); g.connectAudio(insert, oscairn::graphOutput);
        if(p == parts) { g.commit(); kept = g.release(synth); g.remove(insert); }
        else { played.push_back(&g.host(synth)); played.push_back(&g.host(insert)); }
    }
    g.commit();
    g.setVoiceBudget(-1);

    std::vector<float> left(frames), right(frames); float* out[]{left.data(), right.data()};
    const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
    auto block = [&](const std::vector<kick::clap::Event>& e) {
        oscairn::worker::callbackDeadline = std::chrono::steady_clock::now() + std::chrono::nanoseconds(int64_t(double(period.count()) * (1 - oscairn::worker::mixingReserve)));
        g.process(out, frames, nullptr, e.data(), uint32_t(e.size()));
        oscairn::worker::callbackDeadline = {};
    };
    uint64_t missed = 0;
    auto missedNow = [&] { uint64_t n = 0; for(auto* h : played) { const auto s = h->statistics(); n += s.lateBlocks + s.silentBlocks; } return n; };
    // Per block over `blocks`: handed, bytes copied, system calls; and the kept instance's counts, which must not move.
    auto measure = [&](uint32_t blocks) {
        const auto before = count(played), keptBefore = count({kept.get()});
        const auto missedBefore = missedNow();
        for(uint32_t i = 0; i < blocks; ++i) block({});
        missed += missedNow() - missedBefore;
        const auto w = count(played) - before, k = count({kept.get()}) - keptBefore;
        return std::array<double, 4>{w.handed / blocks, w.copied / blocks, w.calls / blocks, k.handed + k.copied + k.calls};
    };
    for(int i = 0; i < 800; ++i) block({}); // the workers started, past their start's grace
    const auto rest = measure(400);
    for(int k = 0; k < 30; ++k)
    {
        std::vector<kick::clap::Event> e;
        for(int p = 0; p < parts; ++p) e.push_back(kick::clap::Event::note(true, 40 + k, .8, 0, int16_t(p)));
        block(e);
    }
    for(int i = 0; i < 100; ++i) block({});
    const auto held = measure(400);

    // The budgets per block (docs/developer-guide.md): raise one only on purpose, saying why in the commit.
    const std::array<double, 3> restBudget{10, 15360, 20}, heldBudget{10, 19380, 20};
    const char* what[]{"blocks handed", "bytes copied", "system calls"};
    std::printf("engine side per block at rest: %.2f handed, %.0f bytes, %.2f calls; keys held: %.2f handed, %.0f bytes, %.2f calls; "
                "kept warm: %.0f at rest, %.0f held; blocks missed %llu\n",
                rest[0], rest[1], rest[2], held[0], held[1], held[2], rest[3], held[3], static_cast<unsigned long long>(missed));
    int failures = 0;
    if(rest[0] == 0) { std::fprintf(stderr, "FAIL: nothing counted: built without OSCAIRN_COST_COUNTERS\n"); return 1; }
    for(int i = 0; i < 3; ++i)
    {
        if(rest[i] > restBudget[i]) { std::fprintf(stderr, "FAIL: at rest, %s per block %.2f over its budget %.0f\n", what[i], rest[i], restBudget[i]); ++failures; }
        if(held[i] > heldBudget[i]) { std::fprintf(stderr, "FAIL: keys held, %s per block %.2f over its budget %.0f\n", what[i], held[i], heldBudget[i]); ++failures; }
    }
    if(rest[2] > 2 * rest[0] || held[2] > 2 * held[0]) { std::fprintf(stderr, "FAIL: more than a wake and a sleep a block handed\n"); ++failures; }
    if(rest[3] != 0 || held[3] != 0) { std::fprintf(stderr, "FAIL: the instance kept warm cost the engine thread work\n"); ++failures; }
    return failures ? 1 : 0;
}
