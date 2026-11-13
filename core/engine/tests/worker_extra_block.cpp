// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's extra block (OSCAIRN_WORKER_EXTRA_BLOCK, experimental): a plugin listed there renders
// each block in its worker while the engine plays the block before, so it sounds exactly one block
// late, sample for sample, with its output events, and the result of each call one call late too;
// notes off and a panic take effect one block late as well. Plugins not listed are not delayed, in
// a worker or in the engine. After its worker restarts, nothing the old worker rendered is played.
// In a graph, the delayed part and the effect after it come out one block late, rendered either way.
//   oscairn-worker-extra-block-test WORKER FIXTURE.clap NOTE-CRASH.clap BASIC-SYNTH.clap
#include <oscairn/graph.h>
#include <oscairn/plugin_host.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <thread>
#include <vector>

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }

    constexpr double rate = 48000; constexpr uint32_t frames = 128;

    struct Block { std::vector<float> l, r; bool ok = false; std::vector<double> values; }; // values: the fixture's gain reports

    // One block on a host, by process() or, as the graph's waves do, startProcess and finishProcess.
    Block render(oscairn::PluginHost& host, const std::vector<kick::clap::Event>& events, bool halves, const float* const* in = nullptr, uint32_t ni = 0)
    {
        Block b{std::vector<float>(frames, 7.f), std::vector<float>(frames, 7.f)}; // not silence: what the host writes is checked
        float* out[]{b.l.data(), b.r.data()};
        if(halves) { check(host.startProcess(in, ni, out, 2, frames, events.data(), uint32_t(events.size()), nullptr), "a block starts"); b.ok = host.finishProcess(); }
        else b.ok = host.process(in, ni, out, 2, frames, events.data(), uint32_t(events.size()), nullptr);
        uint32_t n = 0; const auto* e = host.outputEvents(n);
        for(uint32_t i = 0; i < n; ++i)
            if(e[i].header()->type == CLAP_EVENT_PARAM_VALUE) b.values.push_back(reinterpret_cast<const clap_event_param_value_t*>(e[i].header())->value);
        return b;
    }
    bool silent(const Block& b) { for(size_t i = 0; i < frames; ++i) if(b.l[i] != 0 || b.r[i] != 0) return false; return true; }
    bool same(const Block& a, const Block& b) { return a.l == b.l && a.r == b.r && a.ok == b.ok && a.values == b.values; }

    std::vector<kick::clap::Event> notesAt(uint32_t block)
    {
        std::vector<kick::clap::Event> e; // each at its frame in the block
        if(block == 3) e.push_back(kick::clap::Event::note(true, 60, .8, 37));
        if(block == 9) e.push_back(kick::clap::Event::note(false, 60, 0, 5));
        if(block == 14) e.push_back(kick::clap::Event::note(true, 64, .5, 0));
        if(block == 22) e.push_back(kick::clap::Event::note(true, 67, .5, 100));
        return e;
    }

    // A delayed synth against the same plugin in the engine, one block apart, and an effect in another
    // worker (not listed) against the engine's, block for block; a panic at block 18 on both.
    void shifted(const char* worker, const char* fixture)
    {
        auto reference = oscairn::loadPlugin(fixture, "org.kick.test.synth", rate, frames, oscairn::ProcessingMode::Realtime);
        auto delayed = oscairn::loadPluginInWorker(worker, fixture, "org.kick.test.synth", rate, frames, oscairn::ProcessingMode::Realtime);
        auto effectHere = oscairn::loadPlugin(fixture, "org.kick.test.effect", rate, frames, oscairn::ProcessingMode::Realtime);
        auto effect = oscairn::loadPluginInWorker(worker, fixture, "org.kick.test.effect", rate, frames, oscairn::ProcessingMode::Realtime);
        std::vector<Block> expected;
        std::vector<float> l(frames), r(frames); const float* in[]{l.data(), r.data()};
        bool effectSame = true;
        for(uint32_t block = 0; block < 40; ++block)
        {
            const auto notes = notesAt(block);
            if(block == 18) { reference->panic(); delayed->panic(); }
            expected.push_back(render(*reference, notes, false));
            const auto got = render(*delayed, notes, block % 2 == 1);
            if(block == 0) check(silent(got) && got.ok && got.values.empty(), "the first block is silent, processed, with no events");
            else check(same(got, expected[block - 1]), "block " + std::to_string(block) + " plays block " + std::to_string(block - 1) + ", its audio, events and result");
            for(uint32_t i = 0; i < frames; ++i) { l[i] = float(block * frames + i) / 9000.f; r[i] = -l[i]; }
            effectSame = effectSame && same(render(*effectHere, notes, false, in, 2), render(*effect, notes, block % 2 == 0, in, 2));
        }
        double energy = 0; for(const auto& b : expected) for(float x : b.l) energy += double(x) * x;
        check(energy > 0, "the comparison covers sound");
        check(effectSame, "a plugin not listed renders each block in that block");
        check(expected[9].l[4] != 0 && expected[9].l[5] == 0, "the note ends where it was released (block 9, frame 5)");
        check(delayed->statistics().silentBlocks == 0 && delayed->statistics().lateBlocks == 0, "no block missed with a period more");
        std::printf("extra block: 40 blocks one block late, sample for sample; the effect beside it on time\n");
    }

    // A worker that crashes on a note (key 0) restarts; nothing it rendered before is played after,
    // and a note played after it sounds one block later, as from a fresh instance.
    void restarted(const char* worker, const char* crashing)
    {
        auto delayed = oscairn::loadPluginInWorker(worker, crashing, "org.kick.test.synth", rate, frames, oscairn::ProcessingMode::Realtime);
        auto fresh = oscairn::loadPlugin(crashing, "org.kick.test.synth", rate, frames, oscairn::ProcessingMode::Realtime);
        render(*delayed, {kick::clap::Event::note(true, 60, .8)}, false);
        check(!silent(render(*delayed, {}, true)), "a held note sounds a block later");
        render(*delayed, {kick::clap::Event::note(true, 0, .8)}, false); // the worker aborts on it
        const auto until = std::chrono::steady_clock::now() + std::chrono::seconds(10);
        bool stale = false; uint32_t after = 0;
        while(std::chrono::steady_clock::now() < until && after < 8)
        {
            std::this_thread::sleep_for(std::chrono::microseconds(2667));
            const bool back = delayed->statistics().restarts == 1 && delayed->failure().empty();
            const auto b = render(*delayed, {}, after % 2 == 1);
            if(back) { stale = stale || !silent(b); ++after; } // the note held before the crash is gone with its voices
        }
        check(after == 8, "the worker restarted: " + delayed->failure());
        check(!stale, "after the restart, nothing the old worker rendered is played");
        const auto expected = render(*fresh, {kick::clap::Event::note(true, 62, .8)}, false);
        check(silent(render(*delayed, {kick::clap::Event::note(true, 62, .8)}, false)), "a note after the restart: silent in its block");
        auto got = render(*delayed, {}, true);
        got.values = expected.values; // a restored worker reports its parameter values again; the audio is what is compared
        check(got.l == expected.l && got.r == expected.r, "and sounds the next, as from a fresh instance (fading in after the restart aside, which ends in 10 ms)");
    }

    // A graph: notes into the Basic Synth (listed) into the fixture's effect (not listed), each in a
    // worker, against the same graph without the extra block, in waves and one step at a time.
    void graph(const char* fixture, const char* synth, bool parallel)
    {
        auto build = [&](oscairn::Graph& g) {
            const auto s = g.add(synth), e = g.add(fixture, "org.kick.test.effect");
            g.connectNotes(oscairn::graphInput, s); g.connectAudio(s, e); g.connectAudio(e, oscairn::graphOutput);
            g.setParallel(parallel); g.commit();
        };
        unsetenv("OSCAIRN_WORKER_EXTRA_BLOCK");
        oscairn::Graph plain(rate, frames); build(plain);
        setenv("OSCAIRN_WORKER_EXTRA_BLOCK", "org.kick.test.synth,org.oscairn.basic-synth", 1);
        oscairn::Graph late(rate, frames); build(late);
        std::vector<std::vector<float>> expected;
        bool shiftedSame = true; double energy = 0;
        for(uint32_t block = 0; block < 60; ++block)
        {
            std::vector<float> a(frames * 2), b(frames * 2); float* outA[]{a.data(), a.data() + frames}; float* outB[]{b.data(), b.data() + frames};
            auto notes = notesAt(block);
            plain.process(outA, frames, nullptr, notes.data(), uint32_t(notes.size()));
            late.process(outB, frames, nullptr, notes.data(), uint32_t(notes.size()));
            for(float x : a) energy += double(x) * x;
            if(block == 0) shiftedSame = shiftedSame && std::all_of(b.begin(), b.end(), [](float x) { return x == 0; });
            else shiftedSame = shiftedSame && b == expected.back();
            expected.push_back(a);
        }
        check(energy > 0, "the graph made sound");
        check(shiftedSame, std::string("in a graph (") + (parallel ? "waves" : "one step at a time") + "), the delayed synth and the effect after it come out one block late, sample for sample");
    }
}

int main(int argc, char** argv)
{
    if(argc < 5) { std::fprintf(stderr, "usage: %s WORKER FIXTURE NOTE-CRASH BASIC-SYNTH\n", argv[0]); return 2; }
    // Identical results, not timing: a period more for every block on a loaded machine.
    setenv("OSCAIRN_WORKER_BUDGET_US", "500000", 1);
    setenv("OSCAIRN_WORKER_EXTRA_BLOCK", "org.kick.test.synth,org.oscairn.basic-synth", 1);
    try
    {
        shifted(argv[1], argv[2]);
        restarted(argv[1], argv[3]);
        auto policy = oscairn::placementPolicy(); policy.worker = argv[1]; oscairn::setPlacementPolicy(policy); // every plugin in a worker
        graph(argv[2], argv[4], true);
        graph(argv[2], argv[4], false);
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    return failures ? 1 : 0;
}
