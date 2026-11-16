// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A scene recalled stages a value for every parameter it stored; none may leave a save refused
// ("parameter edits are still pending") once the engine has taken them, whether or not audio is running.
//   oscairn-scene-save-test STEP_SEQUENCER.clap BASIC_SYNTH.clap
#include "oscairn/session.h"
#include <array>
#include <filesystem>
#include <iostream>
#include <string>
#include <unistd.h>
using namespace oscairn;
namespace
{
    constexpr uint32_t frames = 128;
    void check(bool ok, const std::string& why) { if(!ok) throw std::runtime_error(why); }
    struct Audio
    {
        std::array<float, frames> l{}, r{};
        float* out[2]{l.data(), r.data()};
        void run(Session& s) { s.service(); check(s.process(nullptr, 0, out, 2, frames), "render"); }
    };
    // The refusal, or empty when the save is taken.
    std::string saved(Session& s, const std::filesystem::path& to)
    {
        try { s.savePerformance(to.string()); return {}; } catch(const std::exception& e) { return e.what(); }
    }
    void flip(Session& s, NodeId node, bool high)
    {
        for(const auto& p : s.parameters(node))
            if(!(p.flags & CLAP_PARAM_IS_READONLY) && (p.flags & CLAP_PARAM_IS_AUTOMATABLE)) s.setParameter(node, p.id, high ? p.maximum : p.minimum);
    }
    // Two scenes of different values on every parameter of a sequencer and a synth; recalled, then saved.
    void recall(const char* sequencer, const char* synth, const std::filesystem::path& root, bool audio, bool sameScenes)
    {
        Session s(48000, frames);
        const auto q = s.add("seq", sequencer), b = s.add("synth", synth);
        const auto lead = s.addPart("Lead"), groove = s.addPart("Groove");
        s.insertIntoPart(lead, 0, b); s.insertIntoPart(groove, 0, q); s.commit();
        Audio x; x.run(s);
        flip(s, q, false); flip(s, b, false); x.run(s);
        const auto first = s.addScene("One");
        if(!sameScenes) { flip(s, q, true); flip(s, b, true); x.run(s); }
        const auto second = s.addScene("Two");
        for(int i = 0; i < 4; ++i)
        {
            s.selectScene(i % 2 ? second : first);
            if(audio) for(int k = 0; k < 4; ++k) x.run(s); else s.service();
            const auto why = saved(s, root / ((audio ? "run-" : "idle-") + std::string(sameScenes ? "equal-" : "different-") + std::to_string(i)));
            check(why.empty(), std::string(audio ? "running" : "idle") + (sameScenes ? ", equal scenes" : ", different scenes") + ", recall " + std::to_string(i) + ": " + why);
        }
    }
    // A node sharing an instance keeps its bus (RFC 0021): a value asked of its Output bus is taken and the
    // node stays where it is, as a scene stored with another bus recalls it. It is not left pending for good.
    void keptBus(const char* synth, const std::filesystem::path& root)
    {
        Session s(48000, frames);
        const auto first = s.add("first", synth), second = s.shareInstrument("second", first);
        const auto one = s.addPart("One"), two = s.addPart("Two");
        s.insertIntoPart(one, 0, first); s.insertIntoPart(two, 0, second); s.commit();
        Audio x; x.run(s);
        uint32_t bus = CLAP_INVALID_ID; double kept = 0;
        for(const auto& p : s.parameters(second)) if(p.role == OSCAIRN_ROLE_OUTPUT_BUS) { bus = p.id; kept = s.parameter(second, p.id); }
        check(bus != CLAP_INVALID_ID, "a shared node has an Output bus");
        s.setParameter(second, bus, kept == 3 ? 4 : 3);
        for(int i = 0; i < 4; ++i) x.run(s);
        const auto why = saved(s, root / "kept-bus");
        check(why.empty(), "an Output bus value asked of a shared node: " + why);
        check(s.parameter(second, bus) == kept, "the node keeps its bus: " + std::to_string(s.parameter(second, bus)) + " for " + std::to_string(kept));
    }
}
int main(int argc, char** argv)
{
    if(argc != 3) return 2;
    const auto root = std::filesystem::temp_directory_path() / ("oscairn-scene-save-" + std::to_string(getpid()));
    try
    {
        for(const bool audio : {true, false}) for(const bool same : {false, true}) recall(argv[1], argv[2], root, audio, same);
        keptBus(argv[2], root);
        std::filesystem::remove_all(root);
    }
    catch(const std::exception& e) { std::cerr << "FAIL: " << e.what() << '\n'; std::filesystem::remove_all(root); return 1; }
    std::cout << "ok: scene recalls and saves\n";
    return 0;
}
