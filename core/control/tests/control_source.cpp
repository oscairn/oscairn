// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0011's control tree driven by a plugin's param-target lane (RFC 0002): the tree flattened
// into the graph at commit, so the lane's events reach every parameter under the control at
// their sample, in the audio thread. The test fixture's instrument plays exactly its gain while
// a note is held, so its output is the parameter's value at every sample.
// The Oscairn LFO, the first first-party MODULATE sender, drives a control's relative targets as
// CLAP modulation (the fixture adds its gain's modulation to its gain) and absolute ones as values;
// saving never captures what it drives. At 1,000 Hz it still sends every point, four targets
// receive every one, and nothing is dropped; at high rates its square is band-limited. A target
// fuller than its block holds thins the points, counted, without being reset.
//   oscairn-control-source-test SEQUENCER.clap FIXTURE.clap LFO.clap BASIC_SYNTH.clap
#include "oscairn/session.h"
#include "../../plugin-host/src/StateFiles.h"

#include <algorithm>
#include <array>
#include <map>
#include <span>
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
    // transport (it plays from 0 at the first block); `note`: a key down at the first sample.
    struct Take { size_t start = 0; std::array<std::vector<float>, 4> out; /* Main and B, left then right */ float at(int o, size_t sample) const { return out[size_t(o)][sample - start]; } };
    Take render(Session& s, uint32_t block, size_t& position, double seconds = 1, bool note = false)
    {
        std::vector<float> l(block), r(block), bl(block), br(block);
        float* out[] = {l.data(), r.data(), bl.data(), br.data()};
        Take got{position, {}};
        const auto down = kick::clap::Event::midi(0x90, 60, 100);
        for(size_t at = 0; at < size_t(seconds * rate); at += block)
        {
            s.service();
            const bool first = note && at == 0;
            check(s.process(nullptr, 0, out, 4, block, first ? std::span<const kick::clap::Event>(&down, 1) : std::span<const kick::clap::Event>()), "render");
            got.out[0].insert(got.out[0].end(), l.begin(), l.begin() + block);
            got.out[1].insert(got.out[1].end(), bl.begin(), bl.begin() + block);
            got.out[2].insert(got.out[2].end(), r.begin(), r.begin() + block);
            got.out[3].insert(got.out[3].end(), br.begin(), br.begin() + block);
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
    // The LFO's parameters (plugins/lfo/lfo.cpp). Rate is a position on its log scale, 0.01 Hz to
    // 1,357 Hz, as the LFO maps it; its points come every 32 samples of its own count, closer to
    // keep 16 a cycle at high rates.
    enum : clap_id { lfoRate = 1, lfoShape = 4, lfoDepth = 5, lfoMode = 9 };
    double ratePosition(double hz) { return std::log(hz / .01) / std::log(1357 / .01); }
    double rateHz(double position) { return .01 * std::pow(1357 / .01, position); }
    size_t spacingAt(double hz) { return size_t(std::clamp(std::floor(rate / (hz * 16) + 1e-6), 1.0, 32.0)); }

    // The LFO with its parameters set, two fixtures (four with `four`: C and D on Main's and B's right)
    // playing a held key to Main and B, and a control at `value` with the LFO's lane as its source and
    // targets a and b (c as a, d as b).
    struct LfoRig { NodeId lfo, a, b; uint32_t control; };
    LfoRig lfoRig(Session& s, const char* lfo, const char* fixture, double shape, double value, const MacroTarget& a, const MacroTarget& b, double hz = 3, bool four = false)
    {
        LfoRig r{s.add("lfo", lfo), s.add("a", fixture), s.add("b", fixture), 0};
        s.setParameter(r.lfo, lfoRate, ratePosition(hz)); s.setParameter(r.lfo, lfoShape, shape); s.setParameter(r.lfo, lfoDepth, .5);
        std::vector<NodeId> nodes{r.a, r.b};
        if(four) { nodes.push_back(s.add("c", fixture)); nodes.push_back(s.add("d", fixture)); }
        std::vector<MacroTarget> targets;
        for(size_t i = 0; i < nodes.size(); ++i)
        {
            s.connectNotes(graphInput, nodes[i]);
            s.connectAudio(AudioConnection{{nodes[i], 0, 0}, {graphOutput, uint32_t(i % 2), uint32_t(i / 2)}, 1});
            auto t = i % 2 ? b : a; t.node = nodes[i]; t.param = gain;
            targets.push_back(t);
        }
        r.control = s.addMacro("LFO");
        s.setMacroTargets(r.control, targets);
        s.setMacroSource(r.control, r.lfo, 0);
        s.commit();
        s.setMacro(r.control, value);
        return r;
    }

    // The LFO's points, as it computes them: each one's sample and the shape there, sine or
    // triangle, -1..1.
    struct Point { size_t at; double w; };
    std::vector<Point> pointsOf(int shape, double hz, size_t samples)
    {
        std::vector<Point> points;
        const double step = rateHz(ratePosition(hz)) / rate;
        const size_t spacing = spacingAt(rateHz(ratePosition(hz)));
        double phase = 0;
        for(size_t n = 0; n < samples; ++n)
        {
            if(n % spacing == 0) points.push_back({n, shape == 1 ? (phase < .5 ? 4 * phase - 1 : 3 - 4 * phase) : std::sin(2 * 3.14159265358979323846 * phase)});
            phase += step;
            if(phase >= 1) phase -= std::floor(phase);
        }
        return points;
    }

    // Piece 2: the LFO's MODULATE lane through a control at 0.5 to two relative targets (0.3 to 0.7,
    // and reversed) is CLAP modulation on each fixture's gain, from each point to the next; the
    // gains' own values stay at 0.5. At 3 Hz its points are 32 samples apart; at 1,000 Hz, 3 (16 a
    // cycle), each reaching four targets, every sample of each checked, with nothing dropped.
    void lfoModulates(const char* lfo, const char* fixture)
    {
        struct Case { int shape; double hz; bool four; };
        for(const auto c : {Case{0, 3, false}, Case{1, 3, false}, Case{0, 1000, true}})
            for(const uint32_t block : {256u, 37u})
            {
                Session s(rate, maxFrames, {}, twoOutputs);
                const auto r = lfoRig(s, lfo, fixture, c.shape, .5, MacroTarget{.start = .3, .end = .7, .relative = true}, MacroTarget{.start = .7, .end = .3, .relative = true}, c.hz, c.four);
                size_t position = 0;
                const auto got = render(s, block, position, .5, true);
                const auto points = pointsOf(c.shape, c.hz, got.out[0].size());
                const std::string what = std::string(c.shape ? "triangle" : "sine") + " at " + std::to_string(int(c.hz)) + " Hz, blocks of " + std::to_string(block);
                check(spacingAt(rateHz(ratePosition(c.hz))) == (c.hz < 100 ? 32u : 3u), what + ": points " + std::to_string(spacingAt(rateHz(ratePosition(c.hz)))) + " samples apart");
                size_t bad = 0, checked = 0; double peak = 0;
                for(size_t k = 0; k + 1 < points.size(); ++k)
                {
                    const double x = std::clamp(.5 + .5 * points[k].w, 0.0, 1.0);
                    const double a = .5 + (.3 + .4 * x - .5), b = .5 + (.7 - .4 * x - .5);
                    for(size_t at = points[k].at; at < points[k + 1].at; ++at)
                        for(int o = 0; o < (c.four ? 4 : 2); ++o)
                            if(!near(got.at(o, at), o % 2 ? b : a) && !bad++)
                                std::fprintf(stderr, "FAIL: %s: point %zu (sample %zu), output %d: %f; expected %f\n", what.c_str(), k, at, o, got.at(o, at), o % 2 ? b : a);
                    ++checked;
                    peak = std::max(peak, std::abs(got.at(0, points[k].at) - .5));
                }
                check(!bad && checked >= (c.hz < 100 ? 700u : 7900u), what + ": the targets follow the LFO from each of its points to the next (" + std::to_string(bad) + " wrong of " + std::to_string(checked) + ")");
                check(s.sessionStatus().graphEventDrops == 0, what + ": no events dropped: " + std::to_string(s.sessionStatus().graphEventDrops));
                check(near(peak, .2), what + ": a depth of 0.5 swings the control 0.5 either way, its whole travel, each gain 0.2: " + std::to_string(peak));
                check(near(s.parameter(r.a, gain), .5) && near(s.parameter(r.b, gain), .5), what + ": modulation leaves the gains' values at 0.5");
                check(near(s.graphState().macros.front().value, .5), what + ": the control keeps its own value");
                bool listed = false;
                for(const auto& n : s.graphState().topology.nodes) if(n.id == r.lfo) listed = n.plugin.lanes == std::vector<std::pair<uint32_t, std::string>>{{0, "LFO"}};
                check(listed, what + ": the LFO's lane is listed with its name");
            }
    }

    // A square at 3 Hz takes its two values only; at 1,000 Hz polyBLEP puts points between them at
    // each step.
    void lfoBandLimited(const char* lfo, const char* fixture)
    {
        for(const double hz : {3.0, 1000.0})
        {
            Session s(rate, maxFrames, {}, twoOutputs);
            lfoRig(s, lfo, fixture, 3, .5, MacroTarget{.start = .3, .end = .7, .relative = true}, MacroTarget{.start = .7, .end = .3, .relative = true}, hz);
            size_t position = 0;
            const auto got = render(s, 256, position, .1, true);
            const auto between = std::count_if(got.out[0].begin(), got.out[0].end(), [](float v) { return std::abs(v - .3f) > 1e-6 && std::abs(v - .7f) > 1e-6; });
            check(hz < 100 ? between == 0 : between > 100, "a square at " + std::to_string(int(hz)) + " Hz: " + std::to_string(between) + " samples between its two values");
        }
    }

    // At the top rate, 1,357 Hz (a point every 2 samples), in blocks of 512, three controls on the
    // LFO's lane all reaching A's gain bring it 768 points a block, past the 512 a target's block
    // holds: the newest replace the last ones waiting, each counted, and A is never reset (its held
    // note keeps sounding).
    void lfoThinned(const char* lfo, const char* fixture)
    {
        constexpr uint32_t block = 512;
        Session s(rate, block, {}, twoOutputs);
        const auto r = lfoRig(s, lfo, fixture, 0, .5, MacroTarget{.start = .3, .end = .7, .relative = true}, MacroTarget{.start = .7, .end = .3, .relative = true}, 1357);
        for(int i = 0; i < 2; ++i)
        {
            const auto c = s.addMacro("More");
            s.setMacroTargets(c, {MacroTarget{.node = r.a, .param = gain, .start = .3, .end = .7, .relative = true}});
            s.setMacroSource(c, r.lfo, 0);
        }
        s.commit();
        size_t position = 0;
        const auto got = render(s, block, position, .5, true);
        const uint64_t blocks = got.out[0].size() / block, drops = s.sessionStatus().graphEventDrops;
        check(drops > 0 && drops <= 256 * blocks, "a target's full block thins the points, counted: " + std::to_string(drops) + " in " + std::to_string(blocks) + " blocks");
        check(std::none_of(got.out[0].begin(), got.out[0].end(), [](float v) { return v == 0; }), "the full target is never reset: its note sounds throughout");
    }

    // The Basic Synth's Brightness modulated through a relative target: the sound changes, the
    // parameter does not.
    void synthBrightness(const char* lfo, const char* synth)
    {
        auto play = [&](bool driven) {
            Session s(rate, maxFrames);
            const auto l = s.add("lfo", lfo), v = s.add("synth", synth);
            s.setParameter(l, lfoRate, ratePosition(5)); s.setParameter(v, 1, .5);
            s.connectNotes(graphInput, v); s.connectAudio(v, graphOutput);
            const auto c = s.addMacro("Bright");
            s.setMacroTargets(c, {MacroTarget{.node = v, .param = 1, .start = 0, .end = 1, .relative = true}});
            if(driven) s.setMacroSource(c, l, 0);
            s.commit(); s.setMacro(c, .5);
            std::vector<float> left(maxFrames), right(maxFrames), all;
            float* out[] = {left.data(), right.data()};
            const auto down = kick::clap::Event::midi(0x90, 48, 127);
            for(size_t at = 0; at < size_t(rate / 2); at += maxFrames)
            {
                s.service();
                check(s.process(out, maxFrames, at ? std::span<const kick::clap::Event>() : std::span<const kick::clap::Event>(&down, 1)), "render the synth");
                all.insert(all.end(), left.begin(), left.end());
            }
            check(near(s.parameter(v, 1), .5), "Brightness keeps its value under modulation");
            return all;
        };
        const auto still = play(false), moved = play(true);
        double difference = 0, level = 0;
        for(size_t i = 0; i < still.size(); ++i) { difference += std::abs(still[i] - moved[i]); level += std::abs(still[i]); }
        check(level > 0 && difference > .05 * level, "the LFO on Brightness changes the sound: " + std::to_string(difference / std::max(level, 1e-9)));
    }

    // Saving never captures modulation: saved mid-swing, an absolute target loads where the control's
    // own value puts it and a relative one at its value; the same as saving with the LFO removed.
    void savedUnmodulated(const char* lfo, const char* fixture, const std::filesystem::path& root)
    {
        Session s(rate, maxFrames, {}, twoOutputs);
        const auto r = lfoRig(s, lfo, fixture, 1, .5, MacroTarget{.start = .2, .end = .6}, MacroTarget{.start = .3, .end = .7, .relative = true});
        size_t position = 0;
        render(s, 256, position, .1, true); // a triangle at 3 Hz: 0.3 of a cycle, mid-swing
        const double driven = s.parameter(r.a, gain);
        check(std::abs(driven - .4) > .02, "mid-swing the absolute target is driven away from 0.4: " + std::to_string(driven));
        s.savePerformance((root / "swing").string());
        s.remove(r.lfo, UserWires::Remove); s.commit();
        check(near(s.parameter(r.a, gain), .4), "with the LFO gone the absolute target is where the control puts it: " + std::to_string(s.parameter(r.a, gain)));
        s.savePerformance((root / "still").string());

        double loadedRate = 0;
        auto loaded = [&](const char* name) {
            Session t(rate, maxFrames, {}, twoOutputs);
            t.loadPerformance((root / name).string());
            std::map<std::string, double> values;
            for(const auto& [id, n] : t.graphState().names) if(n == "a" || n == "b") values[n] = t.parameter(id, gain); else if(n == "lfo") loadedRate = t.parameter(id, lfoRate);
            values["control"] = t.graphState().macros.front().value;
            return values;
        };
        const auto swing = loaded("swing"), still = loaded("still");
        check(near(swing.at("a"), .4) && near(swing.at("b"), .5), "saved mid-swing, loaded unmodulated: a " + std::to_string(swing.at("a")) + ", b " + std::to_string(swing.at("b")));
        check(near(swing.at("control"), .5), "the LFO's control loads at its own value");
        check(near(loadedRate, ratePosition(3)), "the LFO's parameters are saved: rate " + std::to_string(rateHz(loadedRate)));
        check(swing == still, "loaded the same as when saved with the LFO removed");
    }

}

int main(int argc, char** argv)
{
    if(argc < 5) { std::fprintf(stderr, "usage: %s SEQUENCER FIXTURE LFO BASIC_SYNTH\n", argv[0]); return 2; }
    try
    {
        state_files::TemporaryDirectory temp;
        setLane(argv[1], argv[2], temp.path);
        lfoModulates(argv[3], argv[2]);
        lfoBandLimited(argv[3], argv[2]);
        lfoThinned(argv[3], argv[2]);
        synthBrightness(argv[3], argv[4]);
        savedUnmodulated(argv[3], argv[2], temp.path);
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures) return 1;
    std::printf("ok: control sources\n");
    return 0;
}
