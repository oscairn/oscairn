// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Pad maps: fills, playing a pad through its assignment, releases after a bank change,
// pressure, and saving.
//   oscairn-pads-test FIXTURE.clap BASIC_SYNTH.clap
#include "oscairn/session.h"
#include "oscairn/midi_input.h"
#include "AllocationGuard.h"
#include "../../plugin-host/src/StateFiles.h"
#include <array>
#include <cmath>
#include <cstdio>
#include <fstream>
#include <iterator>
#include <string>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr uint32_t frames = 128;
    struct Audio
    {
        std::array<float, frames> l{}, r{};
        float* out[2]{l.data(), r.data()};
        void run(Session& s) { s.service(); forbidAllocation = true; const bool ok = s.process(out, frames); forbidAllocation = false; check(ok, "render"); }
    };
    std::array<int, 16> notes(const PadMap& m, int bank) { std::array<int, 16> n{}; for(int i = 0; i < 16; ++i) n[size_t(i)] = m.assignments[size_t(bank)][size_t(i)].note; return n; }

    void fills()
    {
        PadMap m;
        check(notes(m, 0)[0] == 36 && notes(m, 0)[15] == 51 && notes(m, 1)[0] == 52, "the default is chromatic, bank A from 36");
        check(notes(m, 7)[0] == (36 + 112) % 128, "the last banks wrap around, so every note has a pad");
        fillPads(m, -1, {PadFill::Kind::Chromatic, 0});
        check(notes(m, 0)[0] == 0 && notes(m, 7)[15] == 127, "the KICK's own mapping: A01 is 0, H16 is 127");
        fillPads(m, 0, {PadFill::Kind::Drums});
        check(notes(m, 0)[0] == 36 && notes(m, 0)[1] == 38 && notes(m, 0)[2] == 42 && notes(m, 1)[0] == 16, "drums fill only the chosen bank");
        // Fourths in C major, bank D (from 60, middle C): rows go up three scale degrees.
        PadFill fourths; fourths.kind = PadFill::Kind::Fourths;
        fillPads(m, 3, fourths);
        check(notes(m, 3)[0] == 60 && notes(m, 3)[1] == 62 && notes(m, 3)[3] == 65 && notes(m, 3)[4] == 65, "in key: the second row starts a fourth up (F)");
        fourths.inKey = false; fillPads(m, 3, fourths);
        check(notes(m, 3)[4] == 65 && notes(m, 3)[5] == 66, "chromatic fourths: five semitones a row, every semitone along it");
        PadFill blues; blues.kind = PadFill::Kind::Sequential; blues.root = 9; blues.scale = 11; // A blues
        fillPads(m, 3, blues);
        check(notes(m, 3)[0] == 69 && notes(m, 3)[1] == 72 && notes(m, 3)[6] == 81, "sequential in A blues wraps into the next octave");
        check(noteName(60) == "C4" && noteName(61) == "C♯4" && noteName(0) == "C-1", "note names, middle C as C4");
        PadMap bad; bad.assignments[0][0].channel = 16;
        check(!bad.valid(), "channel 17 is invalid");
    }

    // Zones tell which synth a pad's note and channel reached: low keys play `low`, channel 3 plays
    // `third`.
    void playing(const char* fixture)
    {
        Session s(48000, frames);
        const auto low = s.add("low", fixture, "org.kick.test.synth"), high = s.add("high", fixture, "org.kick.test.synth"), third = s.add("third", fixture, "org.kick.test.synth");
        s.setParameter(low, 7, .125); s.setParameter(high, 7, .25); s.setParameter(third, 7, .5);
        NoteZone below; below.highKey = 59; below.channels = 1;
        NoteZone above; above.lowKey = 60; above.channels = 1;
        NoteZone ch3; ch3.channels = 1u << 2;
        s.connectNotes(graphInput, low); s.connectNotes(graphInput, high); s.connectNotes(graphInput, third);
        for(const auto [node, zone] : {std::pair{low, below}, std::pair{high, above}, std::pair{third, ch3}})
        {
            for(const auto& w : s.wires()) if(w.kind == WireKind::Notes && w.to == node) s.setZones(w.id, {zone});
            s.connectAudio(node, graphOutput);
        }
        s.commit();
        PadMap map;
        map.assignments[0][0].note = 40;                                     // follows the player's channel
        map.assignments[0][1].note = 72; map.assignments[0][1].channel = 2;  // always channel 3
        map.assignments[1][0].note = 80;
        s.setPadMap(map);
        Audio x; x.run(s);
        s.pad(0, true, .8); x.run(s); check(std::abs(x.l[0] - .125) < 1e-6, "pad 1 plays 40 on the player's channel");
        s.pad(1, true, .8); x.run(s); check(std::abs(x.l[0] - .625) < 1e-6, "pad 2 plays 72 on channel 3 only");
        map.bank = 1; s.setPadMap(map);
        s.pad(0, false, 0); x.run(s); check(std::abs(x.l[0] - .5) < 1e-6, "after a bank change, pad 1's release still ends 40");
        s.pad(0, true, .8); x.run(s); check(std::abs(x.l[0] - .75) < 1e-6, "bank B's pad 1 plays 80");
        s.pad(0, false, 0); s.pad(1, false, 0); x.run(s); check(std::abs(x.l[0]) < 1e-6, "everything released");
        check(s.padMap() == map, "the map reads back");
        bool refused = false; try { s.pad(16, true, 1); } catch(const std::invalid_argument&) { refused = true; }
        check(refused, "pad 17 is refused");
    }

    // Poly and channel pressure brighten Basic Synth; "none" leaves it as without pressure.
    void pressure(const char* synth)
    {
        auto render = [&](PadAssignment::Pressure mode, bool press) {
            Session s(48000, frames);
            const auto n = s.add("synth", synth);
            s.connectNotes(graphInput, n); s.connectAudio(n, graphOutput); s.commit();
            PadMap map; map.assignments[0][0].note = 60; map.assignments[0][0].pressure = mode; s.setPadMap(map);
            Audio x; std::vector<float> all;
            s.pad(0, true, .8);
            for(int i = 0; i < 100; ++i) { if(press && i == 10) s.padPressure(0, 1); x.run(s); all.insert(all.end(), x.l.begin(), x.l.end()); }
            return all;
        };
        const auto plain = render(PadAssignment::Pressure::Poly, false);
        check(render(PadAssignment::Pressure::None, true) == plain, "a pad set to no pressure sends none");
        check(render(PadAssignment::Pressure::Poly, true) != plain, "poly pressure reaches the synth");
        check(render(PadAssignment::Pressure::Channel, true) != plain, "channel pressure reaches the synth");
    }

    // input.aftertouch: poly (the default) passes a key's pressure per note, so only the held key's brightens the held note; channel
    // makes a key's pressure its channel's, so pressure on a key not held still does. A part whose receive setting leaves out aftertouch gets
    // neither. The pressure comes as a keyboard's does (midiToClap's pressure expression), or as a pad's (MIDI 0xA0).
    void aftertouch(const char* synth, const fs::path& root)
    {
        auto play = [&](Session& s, int key, bool pad) {
            if(pad) { PadMap map; map.assignments[0][0].note = key; map.assignments[0][0].pressure = PadAssignment::Pressure::Poly; s.setPadMap(map); }
            Audio x; std::vector<float> all;
            s.note(true, 57, .8);
            if(pad) s.pad(0, true, .01); // nearly silent: what changes is the held note
            const uint8_t bytes[3]{0xA0, uint8_t(key), 127};
            kick::clap::Event e; if(key >= 0) check(midiToClap(bytes, 3, 0, e), "a key pressure");
            for(int i = 0; i < 100; ++i)
            {
                s.service();
                if(key >= 0 && i == 10 && pad) s.padPressure(0, 1);
                forbidAllocation = true;
                const bool ok = key >= 0 && i == 10 && !pad ? s.process(x.out, frames, {&e, 1}) : s.process(x.out, frames);
                forbidAllocation = false; check(ok, "render");
                if(i > 20) all.insert(all.end(), x.l.begin(), x.l.end());
            }
            return all;
        };
        auto render = [&](bool poly, int key, bool receive, bool pad = false) {
            Session s(48000, frames);
            const auto n = s.add("synth", synth);
            const auto p = s.addPart("A"); s.insertIntoPart(p, 0, n); s.commit();
            if(!receive) { auto in = s.parts()[0].input; in.functions = withFunctionReceive(0, Function::Aftertouch, PedalReceive::None); s.setPartInput(p, in); s.commit(); }
            s.setPolyAftertouch(poly);
            check(s.polyAftertouch() == poly, "the mode reads back");
            return play(s, key, pad);
        };
        // A Performance as a file: saved with the mode left alone (nothing written) or set to channel, then opened in a new session.
        auto opened = [&](bool channel, int key) {
            const auto folder = (root / ((channel ? "aftertouch-channel" : "aftertouch-default") + std::to_string(key))).string();
            {
                Session s(48000, frames);
                const auto n = s.add("synth", synth, "org.oscairn.basic-synth");
                const auto p = s.addPart("A"); s.insertIntoPart(p, 0, n); s.commit();
                if(channel) { s.setPolyAftertouch(false); s.commit(); }
                s.savePerformance(folder);
            }
            std::ifstream in(fs::path(folder) / "performance.json"); const std::string document(std::istreambuf_iterator<char>(in), {});
            check(!document.empty() && (document.find(R"("input":{"aftertouch":"channel"})") != std::string::npos) == channel && (document.find(R"("input":{"aftertouch")") != std::string::npos) == channel, "the file names the mode only when it is channel");
            Session t(48000, frames);
            t.loadPerformance(folder);
            check(t.polyAftertouch() == !channel, "a Performance without the field opens as poly, one that says channel as channel");
            return play(t, key, false);
        };
        const auto file = opened(false, -1);
        check(opened(false, 64) == file && opened(false, 57) != file, "a Performance without input.aftertouch passes key pressure per note: a key not held changes nothing, the held key's brightens its note");
        check(opened(true, 64) != opened(true, -1), "a Performance whose input.aftertouch is channel folds: a key not held pressed brightens the held note");
        const auto plain = render(false, -1, true);
        check(render(false, 64, true) != plain, "channel: a key not held pressed brightens the held note");
        check(render(true, 64, true) == plain, "poly: a key not held pressed changes nothing");
        check(render(true, 57, true) != plain, "poly: the held key pressed brightens it");
        check(render(true, 57, false) == plain && render(false, 64, false) == plain, "a part not receiving aftertouch gets neither");
        check(render(false, 64, true, true) != render(true, 64, true, true), "a pad's key pressure (MIDI 0xA0) is its channel's too, so it brightens the held note only in channel mode");
    }

    // The Performance's input.aftertouch and its switches' mode are saved only when not the default, read back, and undone.
    void inputAndSwitches(const char* fixture, const fs::path& root)
    {
        Session s(48000, frames);
        s.add("synth", fixture, "org.kick.test.synth"); s.commit();
        auto document = [&](const char* name) { s.savePerformance((root / name).string()); std::ifstream in(root / name / "performance.json"); return std::string(std::istreambuf_iterator<char>(in), {}); };
        const auto plain = document("plain");
        check(plain.find("\"input\"") == std::string::npos && plain.find("physicalControls") == std::string::npos && plain.find("unlinked") == std::string::npos, "defaults are not stored");
        check(s.polyAftertouch(), "a new Performance's aftertouch is poly");
        s.setPolyAftertouch(false); s.latch("button:2", false); s.latch("footswitch:1", false); s.commit();
        const auto set = document("set");
        check(set.find(R"("input":{"aftertouch":"channel"})") != std::string::npos && set.find(R"("physicalControls":{"button:2":{"latch":false},"footswitch:1":{"latch":false}})") != std::string::npos, "channel and two momentary switches are stored");
        bool refused = false; try { s.latch("knob:1", false); } catch(const std::invalid_argument&) { refused = true; }
        check(refused, "a knob has no latch");
        Session t(48000, frames);
        t.loadPerformance((root / "set").string());
        check(!t.polyAftertouch() && t.momentary() == std::vector<std::string>{"button:2", "footswitch:1"}, "read back");
        t.latch("button:2", true); t.setPolyAftertouch(true); t.commit(); t.service();
        check(t.polyAftertouch() && t.momentary() == std::vector<std::string>{"footswitch:1"}, "set back");
        check(t.undo() && !t.polyAftertouch() && t.momentary().size() == 2, "and undone");
    }

    void saving(const char* fixture, const fs::path& root)
    {
        Session s(48000, frames);
        s.add("synth", fixture, "org.kick.test.synth"); s.commit();
        PadMap map; fillPads(map, 2, {PadFill::Kind::Drums}); map.bank = 2; map.assignments[2][5].velocity = .5; map.assignments[2][5].channel = 9; map.assignments[2][5].pressure = PadAssignment::Pressure::Channel;
        s.setPadMap(map);
        s.savePerformance((root / "saved").string());
        Session t(48000, frames);
        t.loadPerformance((root / "saved").string());
        check(t.padMap() == map, "the pad map round-trips through a Performance");
    }
}

int main(int argc, char** argv)
{
    if(argc < 3) return 2;
    try
    {
        state_files::TemporaryDirectory temp;
        fills();
        playing(argv[1]);
        pressure(argv[2]);
        aftertouch(argv[2], temp.path);
        saving(argv[1], temp.path);
        inputAndSwitches(argv[1], temp.path);
    }
    catch(const std::exception& e) { forbidAllocation = false; std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(!failures) std::puts("ok: pad fills, playing, bank changes, pressure, channel and poly aftertouch, and saving");
    return failures ? 1 : 0;
}
