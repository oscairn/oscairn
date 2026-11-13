// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A note effect that sends what a careless arpeggiator can (hostile_notes_fixture.cpp: floods past
// what the host takes, addresses of -1 and past their range, offs and chokes for notes never
// sent, times one past the block, a choke of every field -1 after a refusal and after reset())
// in front of instruments in parts, as a Performance's arpeggiator parts are: a CLAP-dialect
// instrument, a MIDI-dialect one and a Basic Synth sharing its instance with another. The audio
// thread renders on its own thread with keys played into each block while the control thread
// changes the parts, panics, sets an absurd tempo and opens saved Performances of them, so plans
// and Performances change under the flood. For a sanitizer build to watch; nothing is compared.
//   oscairn-hostile-notes-test HOSTILE.clap FIXTURE.clap BASIC_SYNTH.clap
// OSCAIRN_STRESS=N runs N times as long.
#include "oscairn/session.h"
#include "oscairn/midi_input.h"
#include "../../plugin-host/src/StateFiles.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <optional>
#include <random>
#include <string>
#include <thread>
#include <vector>

using namespace oscairn;

int main(int argc, char** argv)
{
    if(argc != 4) { std::fprintf(stderr, "usage: %s HOSTILE.clap FIXTURE.clap BASIC_SYNTH.clap\n", argv[0]); return 2; }
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } };
    try
    {
        state_files::TemporaryDirectory home, temp;
        ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
        constexpr uint32_t frames = 128;
        Session s(48000, frames);
        auto part = [&](const std::string& name, NodeId instrument) {
            const auto arp = s.add(name + " arp", argv[1], "org.oscairn.test.hostile-notes");
            const auto p = s.addPart(name);
            s.insertIntoPart(p, 0, arp); s.insertIntoPart(p, 1, instrument); s.commit();
            return p;
        };
        part("clap", s.add("clap synth", argv[2], "org.kick.test.synth"));
        part("midi", s.add("midi synth", argv[2], "org.kick.test.midi"));
        const auto basic = s.add("basic", argv[3]);
        part("basic", basic);
        part("shared", s.shareInstrument("basic two", basic));
        const auto saved = temp.path / "hostile";
        s.savePerformance(saved.string());

        std::atomic<bool> stop{false};
        std::atomic<uint64_t> rendered{0};
        std::thread audio([&] {
            std::array<float, frames> l{}, r{};
            float* out[2]{l.data(), r.data()};
            std::mt19937 rng(3);
            std::vector<kick::clap::Event> events;
            std::array<bool, 128> held{};
            while(!stop)
            {
                events.clear();
                for(int k = int(rng() % 3); k-- > 0;)
                {
                    const uint8_t key = uint8_t(rng() % 128), m[3]{uint8_t(held[key] ? 0x80 : 0x90), key, 100};
                    kick::clap::Event e;
                    if(midiToClap(m, 3, uint32_t(rng() % frames), e)) { events.push_back(e); held[key] = !held[key]; }
                }
                std::sort(events.begin(), events.end(), [](const auto& a, const auto& b) { return a.header()->time < b.header()->time; });
                s.process(out, frames, events);
                ++rendered;
                std::this_thread::sleep_for(std::chrono::microseconds(500));
            }
        });

        const char* stress = std::getenv("OSCAIRN_STRESS");
        const int rounds = 40 * std::max(1, stress ? std::atoi(stress) : 1);
        std::mt19937 rng(5);
        std::optional<uint32_t> extra;
        int opened = 0;
        for(int i = 0; i < rounds; ++i)
        {
            try
            {
                switch(rng() % 6)
                {
                case 0: if(extra) { s.removePart(*extra); s.commit(); extra.reset(); } else extra = part("extra " + std::to_string(i), s.add("extra synth " + std::to_string(i), argv[2], "org.kick.test.synth")); break;
                case 1: s.allNotesOff(); break;
                case 2: s.setTempo(rng() % 2 ? 999 : 20); s.setPlaying(rng() % 2); break;
                case 3: s.loadPerformance(saved.string()); extra.reset(); ++opened; break;
                default: break;
                }
            }
            catch(const std::exception& e) { std::fprintf(stderr, "round %d: %s\n", i, e.what()); }
            const auto end = std::chrono::steady_clock::now() + std::chrono::milliseconds(rng() % 60);
            do { s.service(); std::this_thread::sleep_for(std::chrono::milliseconds(2)); } while(std::chrono::steady_clock::now() < end);
        }
        stop = true;
        audio.join();
        s.service();
        check(rendered > 0, "the audio thread rendered");
        check(s.sessionStatus().graphEventDrops > 0, "the flood reached the graph: more than an instrument's block holds, dropped and counted");
        std::printf("%d rounds, %d Performances opened, %llu blocks\n", rounds, opened, (unsigned long long)rendered.load());
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures) return 1;
    std::printf("ok: a hostile note effect's notes in front of instruments\n");
    return 0;
}
