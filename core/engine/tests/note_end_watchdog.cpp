// SPDX-License-Identifier: AGPL-3.0-or-later
// The note-end watchdog: an instrument that never sends NOTE_END (the voice fixture with
// OSCAIRN_FIXTURE_NO_END, its release 1 s) through a connection with a note limit of one. A released
// note keeps its slot while its release sounds: a new note then steals it (a choke for it reaches the
// plugin). Once the plugin has been silent for Graph::noteEndQuiet, the slot is free: a new note takes
// it with nothing stolen, where before the watchdog the dead note held it for ever.
//   oscairn-note-end-watchdog-test VOICES.clap
#include "oscairn/graph.h"
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

using namespace oscairn;
using Event = kick::clap::Event;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    struct Chokes : Graph::Listener
    {
        std::vector<int16_t> keys;
        void events(NodeId, const Event* events, uint32_t count, bool emitted) noexcept override
        {
            if(emitted) return;
            for(uint32_t i = 0; i < count; ++i)
                if(events[i].header()->type == CLAP_EVENT_NOTE_CHOKE) keys.push_back(reinterpret_cast<const clap_event_note_t*>(events[i].header())->key);
        }
    };
}

int main(int argc, char** argv)
{
    if(argc != 2) { std::fprintf(stderr, "usage: %s VOICES.clap\n", argv[0]); return 2; }
    ::setenv("OSCAIRN_FIXTURE_NO_END", "1", 1);
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    Graph g(rate, frames);
    const auto s = g.add(argv[1]);
    NoteZone one; one.maxNotes = 1;
    g.connectNotes(graphInput, s, {one}); g.connectAudio(s, graphOutput); g.commit();
    Chokes chokes; g.setListener(&chokes);
    std::vector<float> l(frames), r(frames); float* out[2]{l.data(), r.data()};
    auto play = [&](double seconds, std::vector<Event> events = {}) {
        for(int i = 0; i < int(seconds * rate / frames); ++i) g.process(out, frames, nullptr, i ? nullptr : events.data(), i ? 0 : uint32_t(events.size()));
    };
    // Sounding: released at once, a new note half a second into its 1 s release steals it.
    play(.1, {Event::note(true, 60, 1)});
    play(.5, {Event::note(false, 60, 0)});
    play(.1, {Event::note(true, 62, 1)});
    check(chokes.keys.size() == 1 && chokes.keys[0] == 60, "a release still sounding keeps its slot: the next note steals it (" + std::to_string(chokes.keys.size()) + " chokes)");
    // Silent: 62 released, its 1 s release over, then noteEndQuiet of silence; a new note steals nothing.
    play(.1, {Event::note(false, 62, 0)});
    play(1.2 + Graph::noteEndQuiet + .2);
    chokes.keys.clear();
    play(.1, {Event::note(true, 64, 1)});
    check(chokes.keys.empty(), "a dead note's slot is freed once its plugin is silent: nothing stolen (" + std::to_string(chokes.keys.size()) + " chokes, the first for key " + (chokes.keys.empty() ? std::string("-") : std::to_string(chokes.keys[0])) + ")");
    g.setListener(nullptr);
    if(!failures) std::printf("ok: a note no plugin ended is let go once its plugin is silent, and not while it sounds\n");
    return failures ? 1 : 0;
}
