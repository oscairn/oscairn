// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0011's control tree driven by a plugin's param-target lane (RFC 0002): the tree flattened
// into the graph at commit, so the lane's events reach every parameter under the control at
// their sample, in the audio thread. The test fixture's instrument plays exactly its gain while
// a note is held, so its output is the parameter's value at every sample.
//   oscairn-control-source-test SEQUENCER.clap FIXTURE.clap
#include "oscairn/session.h"
#include "../../plugin-host/src/StateFiles.h"

#include <array>
#include <cmath>
#include <cstdio>
#include <string>
#include <vector>

using namespace oscairn;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    bool near(double a, double b) { return std::abs(a - b) < 1e-6; }

    constexpr double rate = 48000;
    constexpr uint32_t maxFrames = 256;
    constexpr clap_id gain = 7; // the fixture's
    constexpr size_t step = 6000, gate = 3000; // a sixteenth at 120 BPM, and its gate
    // The sequencer's Lock A values (plugins/step-sequencer/step_sequencer.cpp).
    constexpr std::array<double, 16> locks{.8, .2, .5, .35, .8, .2, .5, .35, .65, .15, .4, .25, .65, .15, .4, .25};
    // Two outputs: Main (the fixture A) and B (the fixture B), each fixture's left channel on its own.
    const AudioLayout twoOutputs{{}, {{0, "Main", 2, 0}, {1, "B", 2, 0}}};

    struct Rig { NodeId sequencer, a, b; uint32_t control; };
    Rig build(Session& s, const char* sequencer, const char* fixture)
    {
        Rig r{s.add("seq", sequencer), s.add("a", fixture), s.add("b", fixture), 0};
        s.connectNotes(r.sequencer, r.a); s.connectNotes(r.sequencer, r.b);
        s.connectAudio(AudioConnection{{r.a, 0, 0}, {graphOutput, 0, 0}, 1});
        s.connectAudio(AudioConnection{{r.b, 0, 0}, {graphOutput, 1, 0}, 1});
        r.control = s.addMacro("Lock");
        MacroTarget toA{.node = r.a, .param = gain, .start = .2, .end = .6};
        MacroTarget toB{.node = r.b, .param = gain, .start = .6, .end = .2}; // reversed
        s.setMacroTargets(r.control, {toA, toB});
        s.commit();
        return r;
    }

    // Each output's left channel over `seconds`, in blocks of `block`, from sample `start` of the
    // transport (it plays from 0 at the first block).
    struct Take { size_t start = 0; std::array<std::vector<float>, 2> out; float at(int o, size_t sample) const { return out[size_t(o)][sample - start]; } };
    Take render(Session& s, uint32_t block, size_t& position, double seconds = 1)
    {
        std::vector<float> l(maxFrames), r(maxFrames), bl(maxFrames), br(maxFrames);
        float* out[] = {l.data(), r.data(), bl.data(), br.data()};
        Take got{position, {}};
        for(size_t at = 0; at < size_t(seconds * rate); at += block)
        {
            s.service();
            check(s.process(nullptr, 0, out, 4, block), "render");
            got.out[0].insert(got.out[0].end(), l.begin(), l.begin() + block);
            got.out[1].insert(got.out[1].end(), bl.begin(), bl.begin() + block);
            position += block;
        }
        return got;
    }

    // Every step the take holds whole, from its first sample: A at a(lock), B at b(lock), silent before.
    bool follows(const Take& got, double (*a)(double), double (*b)(double), const std::string& what)
    {
        size_t checked = 0;
        for(size_t k = (got.start + step - 1) / step; (k * step + gate) <= got.start + got.out[0].size(); ++k)
        {
            const double lock = locks[k % locks.size()];
            const size_t at = k * step;
            if(!near(got.at(0, at), a(lock)) || !near(got.at(0, at + gate - 1), a(lock)) || !near(got.at(1, at), b(lock)) || !near(got.at(1, at + gate - 1), b(lock)) ||
               (at > got.start && (got.at(0, at - 1) != 0 || got.at(1, at - 1) != 0)))
            {
                std::fprintf(stderr, "FAIL: %s: step %zu: A %f, B %f; expected %f, %f\n", what.c_str(), k, got.at(0, at), got.at(1, at), a(lock), b(lock));
                ++failures;
                return false;
            }
            ++checked;
        }
        check(checked >= 5, what + ": steps checked");
        return true;
    }

    // Piece 1: a SET lane drives the control as a knob would, its two targets (one reversed) at
    // the lane's samples; the summing rule with a knob's relative target; loops refused; saved,
    // loaded and undone.
    void setLane(const char* sequencer, const char* fixture, const std::filesystem::path& root)
    {
        for(const uint32_t block : {256u, 37u})
        {
            Session s(rate, maxFrames, {}, twoOutputs);
            const auto r = build(s, sequencer, fixture);
            s.setMacroSource(r.control, r.sequencer, 0); s.commit();
            check(s.graphState().macros.front().sourceNode == r.sequencer, "the control's source is the sequencer");
            s.setPlaying(true);
            size_t position = 0;
            const std::string b = std::to_string(block);
            follows(render(s, block, position), [](double x) { return .2 + .4 * x; }, [](double x) { return .6 - .4 * x; }, "a SET lane through the control, blocks of " + b);

            // Another control, a knob, adds its relative target on A: +0.1 at full (start .4, end .6).
            const auto knob = s.addMacro("Knob");
            s.setMacroTargets(knob, {MacroTarget{.node = r.a, .param = gain, .start = .4, .end = .6, .relative = true}}); s.commit();
            s.setMacro(knob, 1); // played, not an edit: the lane's targets follow it without a commit
            follows(render(s, block, position), [](double x) { return .3 + .4 * x; }, [](double x) { return .6 - .4 * x; }, "the summing rule beside a knob, blocks of " + b);
        }

        Session s(rate, maxFrames, {}, twoOutputs);
        const auto r = build(s, sequencer, fixture);
        // Loops: a lane reaching its own plugin, through the control or the graph.
        bool refused = false;
        try { s.setMacroSource(r.control, r.a, 0); } catch(const std::invalid_argument&) { refused = true; }
        check(refused, "a lane driving its own plugin's parameter is refused");
        s.setMacroSource(r.control, r.sequencer, 0); s.commit();
        refused = false;
        try { s.setMacroTargets(r.control, {MacroTarget{.node = r.sequencer, .param = s.parameters(r.sequencer).back().id}}); } catch(const std::invalid_argument&) { refused = true; }
        check(refused, "a target on the lane's own plugin is refused");
        refused = false;
        s.mapParameter(r.a, 0, r.sequencer, s.parameters(r.sequencer).front().id); // the fixture's lane 0 back to the sequencer
        try { s.setMacroSource(r.control, r.sequencer, 1); } catch(const std::invalid_argument&) { refused = true; }
        check(refused, "a lane reaching its own plugin through the graph is refused");
        s.unmapParameter(r.a, 0); s.commit();

        // Saved and loaded with the Performance.
        const auto saved = root / "lane";
        s.savePerformance(saved.string());
        {
            Session t(rate, maxFrames, {}, twoOutputs);
            t.loadPerformance(saved.string());
            const auto& m = t.graphState().macros;
            check(m.size() == 1 && m.front().sourceNode && m.front().sourceLane == 0 && t.graphState().names.at(m.front().sourceNode) == "seq", "the source is saved and loaded");
            t.setPlaying(true);
            size_t position = 0;
            follows(render(t, 256, position), [](double x) { return .2 + .4 * x; }, [](double x) { return .6 - .4 * x; }, "loaded");
        }

        // Undone: the knob's value plays again (the fixture's default gain, .5).
        check(s.undo(), "undo");
        check(s.graphState().macros.front().sourceNode == 0, "undo takes the source away");
        s.setPlaying(true);
        size_t position = 0;
        const auto got = render(s, 256, position, .2);
        check(near(got.at(0, 0), .5) && near(got.at(1, 0), .5), "undone, the lane drives nothing: " + std::to_string(got.at(0, 0)));
    }
}

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: %s SEQUENCER FIXTURE\n", argv[0]); return 2; }
    try
    {
        state_files::TemporaryDirectory temp;
        setLane(argv[1], argv[2], temp.path);
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures) return 1;
    std::printf("ok: control sources\n");
    return 0;
}
