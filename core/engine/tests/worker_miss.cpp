// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's miss policy: one long block in a worker (the fixture's KICK_FIXTURE_STALL: 100 ms in its
// 30th block, against a 10 ms allowance) costs only a dropout of that part. The block it stalls in is
// silenced after the allowance and the blocks after it while it is busy are not handed over, but no
// panic follows: a note held through it sounds afterwards; a note off sent while it is busy takes
// effect afterwards (a release tail that decays); a note on sent then sounds afterwards; the dropout
// fades out and in with no step larger than about 1 ms's share of the sound; two parts sharing one
// Basic Synth both keep their notes; with the extra block too. The carry of a busy worker's events is
// one block's: past it, the oldest events that are not releases go and a release kept takes effect,
// and only a release lost makes the block one whose events were lost.
//   oscairn-worker-miss-test WORKER STALLING-WORKER FIXTURE.clap BASIC-SYNTH.clap
#include <oscairn/graph.h>
#include <oscairn/plugin_host.h>
#include <oscairn/sound_context.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <functional>
#include <string>
#include <thread>
#include <vector>

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }

    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    constexpr float gain = .5f; // the fixture's sound while a note is held, in its own output
    using kick::clap::Event;

    // One block of a run: its left channel, and whether the worker was busy (the block not handed) or silenced.
    struct Block { std::vector<float> l; bool busy = false, silent = false; };
    struct Run { std::vector<Block> blocks; kick::clap::Statistics stats; int firstBusy = -1, lastSilent = -1; };
    float peak(const Block& b) { float p = 0; for(float x : b.l) p = std::max(p, std::abs(x)); return p; }

    // 200 blocks of a graph's node, about 1 ms apart so the stall's real time passes; `events` gives each
    // block's, knowing whether the worker was busy in the block before.
    Run play(oscairn::Graph& g, oscairn::NodeId node, const std::function<std::vector<Event>(int, bool)>& events, oscairn::PluginHost* host = nullptr)
    {
        auto& h = host ? *host : g.host(node);
        Run r;
        for(int i = 0; i < 200; ++i)
        {
            const auto before = h.statistics();
            const auto e = events(i, r.firstBusy >= 0 && r.blocks.back().busy);
            std::vector<float> l(frames), right(frames); float* out[]{l.data(), right.data()};
            g.process(out, frames, nullptr, e.data(), uint32_t(e.size()));
            const auto after = h.statistics();
            Block b{l, after.busyBlocks > before.busyBlocks, after.silentBlocks > before.silentBlocks};
            if(b.busy && r.firstBusy < 0) r.firstBusy = i;
            if(b.silent) r.lastSilent = i;
            r.blocks.push_back(std::move(b));
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }
        r.stats = h.statistics();
        return r;
    }
    Run play(const char* fixture, const char* id, const std::function<std::vector<Event>(int, bool)>& events)
    {
        oscairn::Graph g(rate, frames);
        const auto n = g.add(fixture, id);
        g.connectNotes(oscairn::graphInput, n); g.connectAudio(n, oscairn::graphOutput); g.commit();
        return play(g, n, events);
    }
    // The stall happened as set: a block silenced after waiting, then blocks not handed, then blocks heard again.
    bool stalled(const Run& r, const std::string& name)
    {
        const bool ok = r.firstBusy > 0 && r.stats.busyBlocks >= 10 && r.stats.silentBlocks >= r.stats.busyBlocks + 1 && r.lastSilent < 180;
        check(ok, name + ": the stall silenced a block and the worker was busy for the next: silent " + std::to_string(r.stats.silentBlocks) + ", busy " + std::to_string(r.stats.busyBlocks));
        std::printf("%s: silent %llu, busy %llu, panics after a miss %llu, dropped events %llu\n", name.c_str(), (unsigned long long)r.stats.silentBlocks,
                    (unsigned long long)r.stats.busyBlocks, (unsigned long long)r.stats.missPanics, (unsigned long long)r.stats.droppedEvents);
        return ok;
    }
    // The largest step between neighboring samples from block `from` on.
    float steepest(const Run& r, size_t from)
    {
        float step = 0, previous = r.blocks[from].l[0];
        for(size_t i = from; i < r.blocks.size(); ++i) for(float x : r.blocks[i].l) { step = std::max(step, std::abs(x - previous)); previous = x; }
        return step;
    }

    // A note held through the stall sounds after it, with no panic, faded out and in.
    void held(const char* fixture, const std::string& name)
    {
        const auto r = play(fixture, "org.kick.test.synth", [](int i, bool) { return i == 5 ? std::vector<Event>{Event::note(true, 60, .8)} : std::vector<Event>{}; });
        if(!stalled(r, name)) return;
        check(r.stats.missPanics == 0, name + ": no panic after the missed blocks: " + std::to_string(r.stats.missPanics));
        const float level = peak(r.blocks[20]); // the note's level in the graph's output, before the stall
        check(level > 0 && peak(r.blocks.back()) == level && r.blocks.back().l.front() == level, name + ": the note held through the stall sounds after it: " + std::to_string(peak(r.blocks.back())));
        // 1 ms is 48 frames: the fade's steps are the sound's 48th, against the whole sound for a cut.
        const float bound = level / 48 * 1.01f, step = steepest(r, 10); // past the note on and the worker's 10 ms first fade-in
        check(step <= bound, name + ": the dropout fades out and in, no step larger than " + std::to_string(bound) + ": " + std::to_string(step));
        bool faded = false; // in the block silenced after waiting, played in the first busy block with the extra block
        for(int i : {r.firstBusy - 1, r.firstBusy}) for(float x : r.blocks[size_t(i)].l) faded = faded || (x > 0 && x < level);
        check(faded, name + ": the silenced block fades the sound out");
    }

    // A note off sent while the worker is busy takes effect after it: the tail instrument's release decays to silence.
    void released(const char* fixture, const std::string& name)
    {
        int sent = -1;
        const auto r = play(fixture, "org.kick.test.tail", [&](int i, bool busy) {
            if(i == 5) return std::vector<Event>{Event::note(true, 60, .8)};
            if(busy && sent < 0) { sent = i; return std::vector<Event>{Event::note(false, 60, 0, 17)}; }
            return std::vector<Event>{};
        });
        if(!stalled(r, name)) return;
        check(sent > 0 && r.blocks[size_t(sent)].busy, name + ": the note off went into a block not handed over");
        check(r.stats.missPanics == 0, name + ": no panic after the missed blocks");
        // The tail decays over 1,024 frames from the block heard after the stall.
        const float level = peak(r.blocks[20]);
        bool decaying = false;
        for(size_t i = size_t(r.lastSilent) + 1; i < std::min(r.blocks.size(), size_t(r.lastSilent) + 12); ++i)
            for(float x : r.blocks[i].l) decaying = decaying || (x > level * .02f && x < level * .9f);
        check(decaying, name + ": the release decays after the stall, not cut by a panic");
        check(peak(r.blocks.back()) == 0, name + ": and ends: " + std::to_string(peak(r.blocks.back())));
    }

    // A note on sent while the worker is busy sounds after it.
    void started(const char* fixture, const std::string& name)
    {
        int sent = -1;
        const auto r = play(fixture, "org.kick.test.synth", [&](int i, bool busy) {
            if(busy && sent < 0) { sent = i; return std::vector<Event>{Event::note(true, 60, .8, 90)}; }
            return std::vector<Event>{};
        });
        if(!stalled(r, name)) return;
        check(sent > 0 && r.blocks[size_t(sent)].busy, name + ": the note on went into a block not handed over");
        check(r.stats.missPanics == 0, name + ": no panic after the missed blocks");
        check(peak(r.blocks.back()) > 0 && r.blocks.back().l.front() == peak(r.blocks.back()), name + ": the note on sent during the stall sounds after it: " + std::to_string(peak(r.blocks.back())));
    }

    // Two parts sharing one Basic Synth in a worker that stalls a block: both keep their notes.
    void shared(const char* stalling, const char* synth)
    {
        setenv("OSCAIRN_TEST_WORKER_STALL", "30:100000", 1);
        auto plugin = oscairn::loadPluginInWorker(stalling, synth, "", rate, frames);
        unsetenv("OSCAIRN_TEST_WORKER_STALL");
        auto engine = std::make_shared<oscairn::SoundContext::Engine>(); engine->host = plugin; engine->busParameter = oscairn::SoundContext::busParameter(*plugin);
        const auto sa = plugin->prepareSound("A", {}, {}, {}, {}, {}, {}), sb = plugin->prepareSound("B", {}, {}, {}, {}, {}, {});
        auto ca = std::make_shared<oscairn::SoundContext>(engine, 0, 1, sa), cb = std::make_shared<oscairn::SoundContext>(engine, 1, 2, sb);
        oscairn::Graph g(rate, frames);
        const auto a = g.add(ca), b = g.add(cb);
        oscairn::NoteZone low, high; low.highKey = 63; high.lowKey = 64;
        g.connectNotes(oscairn::graphInput, a, {low}); g.connectNotes(oscairn::graphInput, b, {high});
        g.connectAudio(a, oscairn::graphOutput); g.connectAudio(b, oscairn::graphOutput); g.commit();
        float peakA = 0, peakB = 0;
        const auto r = play(g, a, [&](int i, bool) {
            if(i == 150) { g.takePeaks(a); g.takePeaks(b); } // from here on, well after the stall
            return i == 5 ? std::vector<Event>{Event::note(true, 40, .8), Event::note(true, 90, .8)} : std::vector<Event>{};
        }, plugin.get());
        peakA = g.takePeaks(a)[0]; peakB = g.takePeaks(b)[0];
        if(!stalled(r, "shared")) return;
        check(r.stats.missPanics == 0, "shared: no panic after the missed blocks");
        check(peakA > 0 && peakB > 0, "shared: both parts' held notes sound after the stall: " + std::to_string(peakA) + ", " + std::to_string(peakB));
    }

    // The carry is one block's events. Past it the oldest that are not releases go, and the release kept
    // takes effect; a release that does not fit is a block whose events were lost.
    void overflow(const char* worker, const char* fixture)
    {
        for(const bool releases : {false, true})
        {
            const std::string name = releases ? "carry full of releases" : "carry overflowing";
            auto h = oscairn::loadPluginInWorker(worker, fixture, "org.kick.test.tail", rate, frames);
            std::vector<float> l(frames), r(frames); float* out[]{l.data(), r.data()};
            int busyBlocks = 0, heardAfter = 0; bool lost = false, decaying = false;
            for(int i = 0; i < 200; ++i)
            {
                std::vector<Event> e;
                if(i == 5) e.push_back(Event::note(true, 60, .8));
                const bool busy = busyBlocks > 0 && busyBlocks < 4;
                if(busy && busyBlocks == 1 && !releases) e.push_back(Event::note(false, 60, 0, 3)); // first, so the oldest
                if(busy) for(uint32_t k = 0; k < 600; ++k) e.push_back(releases ? Event::midi(0x80, uint8_t(k % 128), 0, 5) : Event::midi(0xb0, 1, uint8_t(k % 128), 5));
                const auto before = h->statistics().busyBlocks;
                const bool ok = h->process(nullptr, 0, out, 2, frames, e.data(), uint32_t(e.size()), nullptr);
                if(h->statistics().busyBlocks > before) ++busyBlocks;
                else if(busyBlocks) { ++heardAfter; for(float x : l) decaying = decaying || (heardAfter < 12 && x > .01f && x < gain * .9f); }
                if(!ok && busy && !h->eventsKept()) lost = true;
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            const auto s = h->statistics();
            std::printf("%s: busy %d, dropped events %llu\n", name.c_str(), busyBlocks, (unsigned long long)s.droppedEvents);
            check(busyBlocks >= 4, name + ": the events went into blocks not handed over");
            check(s.droppedEvents >= 1800 - 1057, name + ": the carry holds one block's events, the rest dropped: " + std::to_string(s.droppedEvents));
            if(releases) check(lost, name + ": a release that does not fit makes a block whose events were lost");
            else
            {
                check(!lost, name + ": dropping events that are not releases loses nothing a reset is due for");
                check(decaying && l[frames - 1] == 0, name + ": the note off kept takes effect after the stall, the oldest of the rest dropped");
            }
        }
    }
}

int main(int argc, char** argv)
{
    if(argc != 5) { std::fprintf(stderr, "usage: %s WORKER STALLING-WORKER FIXTURE BASIC-SYNTH\n", argv[0]); return 2; }
    // Times far from a busy machine's noise: a 10 ms allowance, a 100 ms stall (under the 250 ms hang limit).
    setenv("OSCAIRN_WORKER_BUDGET_US", "10000", 1);
    setenv("KICK_FIXTURE_STALL", "30:100000", 1);
    try
    {
        overflow(argv[1], argv[3]);
        auto policy = oscairn::placementPolicy(); policy.worker = argv[1]; oscairn::setPlacementPolicy(policy); // every plugin in a worker
        held(argv[3], "held");
        released(argv[3], "released");
        started(argv[3], "started");
        shared(argv[2], argv[4]);
        // The extra block plays each block a period later and waits a period more: the same policy.
        setenv("OSCAIRN_WORKER_EXTRA_BLOCK", "org.kick.test.synth,org.kick.test.tail", 1);
        held(argv[3], "held, extra block");
        released(argv[3], "released, extra block");
        started(argv[3], "started, extra block");
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    return failures ? 1 : 0;
}
