// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Switching Performances as the engine does it, many times over, for a sanitizer build to watch:
// the audio thread rendering on its own thread with keys held and played into each block, while
// the control thread opens, prepares and refuses Performances in quick succession, so one change
// comes while the last still rings out or is being prepared. Among them: two sharing a Basic Synth
// (native sound contexts), Delays and plain CLAP instruments, one of many instruments the budget
// refuses, cuts ("opened at once") and a budget changed between switches. With plugins in workers,
// a thread of its own also kills a worker now and then, so failures and restarts race the loads.
//   oscairn-performance-switch-stress-test SYNTH.clap DELAY.clap INSTRUMENT.clap
// OSCAIRN_STRESS=N switches N times as often.
#include "oscairn/session.h"
#include "oscairn/midi_input.h"
#include "oscairn/plugin_host.h"
#include "../../plugin-host/src/StateFiles.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <csignal>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <random>
#include <string>
#include <thread>
#include <vector>
#ifndef _WIN32
#include <unistd.h>
#endif

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr uint32_t frames = 128;

    void render(Session& s, int n)
    {
        std::array<float, frames> l{}, r{};
        float* out[2]{l.data(), r.data()};
        for(int i = 0; i < n; ++i) { s.service(); s.process(out, frames); }
    }
    // `parts` parts, each a quarter of the keyboard, alternately a Basic Synth playing a sound of its
    // own and a plain CLAP instrument, each through a Delay.
    void save(const fs::path& file, int variant, uint32_t parts, const char* synth, const char* delay, const char* instrument)
    {
        Session s(48000, frames);
        for(uint32_t i = 0; i < parts; ++i)
        {
            const auto name = std::to_string(variant) + "-" + std::to_string(i);
            const auto n = s.add("instrument " + name, i % 2 ? instrument : synth), fx = s.add("delay " + name, delay);
            PartInput keys; keys.zones = {NoteZone{uint8_t(32 * (i % 4)), uint8_t(32 * (i % 4) + 31)}};
            const auto part = s.addPart("Part " + name, keys);
            s.insertIntoPart(part, 0, n); s.insertIntoPart(part, 1, fx); s.commit();
            if(i % 2) check(s.setParameter(n, 7, .3 + .1 * variant), "instrument gain");
            else check(s.selectSound(n, s.prepareSound(n, "Saw " + name, {{4, double(variant % 3)}, {5, double(3 + i % 4)}})), "select a sound");
            check(s.setParameter(fx, 1, .1 + .03 * variant) && s.setParameter(fx, 2, .6) && s.setParameter(fx, 3, .5), "delay settings");
        }
        render(s, 2);
        s.savePerformance(file.string());
    }
}

int main(int argc, char** argv)
{
    if(argc != 4) { std::fprintf(stderr, "usage: %s SYNTH.clap DELAY.clap INSTRUMENT.clap\n", argv[0]); return 2; }
    try
    {
        state_files::TemporaryDirectory home; // a budget's measurements, not the user's
        ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
        state_files::TemporaryDirectory temp;
        std::vector<fs::path> performances;
        for(int v = 0; v < 3; ++v) { performances.push_back(temp.path / ("p" + std::to_string(v))); save(performances.back(), v, 4, argv[1], argv[2], argv[3]); }
        const auto heavy = temp.path / "heavy"; save(heavy, 3, 12, argv[1], argv[2], argv[3]);
        performances.push_back(heavy);

#ifndef _WIN32
        // A Performance whose workers were given up (on the unit: failed as too slow, again and
        // again): the next one opens on instances of its own, not into the failed shared engines and
        // effect engines, which refused every later Performance using those plugins ("cannot restore
        // ... not restarted").
        if(!placementPolicy().worker.empty())
        {
            Session g(48000, frames);
            g.loadPerformance(performances[0].string());
            std::string why;
            for(int kill = 0; kill < 8 && why.find("not restarted") == std::string::npos; ++kill)
            {
                if(FILE* list = popen(("pgrep -P " + std::to_string(::getpid()) + " -f oscairn-plugin-worker").c_str(), "r"))
                {
                    char line[32];
                    while(std::fgets(line, sizeof line, list)) ::kill(std::atoi(line), SIGKILL);
                    pclose(list);
                }
                for(int i = 0; i < 300 && why.find("not restarted") == std::string::npos; ++i)
                {
                    std::this_thread::sleep_for(std::chrono::milliseconds(10));
                    const auto f = g.sessionStatus().failures;
                    why = f.empty() ? "" : f[0].reason;
                    if(i > 20 && why.empty()) break;
                }
            }
            check(why.find("not restarted") != std::string::npos, "the first Performance's workers given up: " + why);
            std::string refused;
            try { g.loadPerformance(performances[1].string()); } catch(const std::exception& e) { refused = e.what(); }
            check(refused.empty(), "the next Performance opens past its failed instances: " + refused);
        }
#endif

        Session s(48000, frames);
        std::atomic<bool> stop{false};
        std::atomic<uint64_t> rendered{0};
        // The audio thread, as the engine's: keys played into each block from a "keyboard".
        std::thread audio([&] {
            std::array<float, frames> l{}, r{};
            float* out[2]{l.data(), r.data()};
            std::mt19937 rng(7);
            std::vector<kick::clap::Event> events;
            std::array<bool, 128> held{};
            while(!stop)
            {
                events.clear();
                for(int k = int(rng() % 4); k-- > 0;)
                {
                    const uint8_t key = uint8_t(rng() % 128);
                    const uint8_t m[3]{uint8_t(held[key] ? 0x80 : 0x90), key, uint8_t(1 + rng() % 127)};
                    kick::clap::Event e;
                    if(midiToClap(m, 3, uint32_t(rng() % frames), e)) { events.push_back(e); held[key] = !held[key]; }
                }
                std::sort(events.begin(), events.end(), [](const auto& a, const auto& b) { return a.header()->time < b.header()->time; });
                s.process(out, frames, events);
                ++rendered;
                std::this_thread::sleep_for(std::chrono::microseconds(300));
            }
        });

        // Workers failing under the loads, as a worker too slow on the unit is failed and restarted.
        std::atomic<int> killed{0};
        std::thread killer([&] {
#ifndef _WIN32
            if(placementPolicy().worker.empty()) return;
            std::mt19937 rng(13);
            while(!stop)
            {
                std::this_thread::sleep_for(std::chrono::milliseconds(300 + rng() % 1200));
                std::vector<int> pids;
                if(FILE* list = popen(("pgrep -P " + std::to_string(::getpid()) + " -f oscairn-plugin-worker").c_str(), "r"))
                {
                    char line[32];
                    while(std::fgets(line, sizeof line, list)) pids.push_back(std::atoi(line));
                    pclose(list);
                }
                if(!pids.empty() && ::kill(pids[rng() % pids.size()], SIGKILL) == 0) ++killed;
            }
#endif
        });
        // Whatever throws below, the threads stop first, so the failure is reported, not std::terminate.
        struct Join { std::atomic<bool>& stop; std::thread& audio; std::thread& killer; ~Join() { stop = true; if(audio.joinable()) audio.join(); if(killer.joinable()) killer.join(); } } join{stop, audio, killer};
        const char* stress = std::getenv("OSCAIRN_STRESS");
        const int switches = 30 * std::max(1, stress ? std::atoi(stress) : 1);
        std::mt19937 rng(11);
        int opened = 0, refused = 0, prepared = 0;
        auto serve = [&](int ms) { const auto end = std::chrono::steady_clock::now() + std::chrono::milliseconds(ms); do { s.service(); std::this_thread::sleep_for(std::chrono::milliseconds(2)); } while(std::chrono::steady_clock::now() < end); };
        for(int i = 0; i < switches; ++i)
        {
            const auto& to = performances[rng() % performances.size()];
            try
            {
                switch(rng() % 6)
                {
                case 0: s.setBudget({}); break;
                case 1: s.setBudget({4, 0}); break; // generous: changes overlap, ringing out
                case 2: s.setBudget({4, s.estimate("").memory + s.estimate(to.string()).memory - 1}); break; // a cut: opened at once
                case 3: s.setBudget({.02, 0}); break; // the heavy one, and maybe the others, refused
                default: break;
                }
                if(rng() % 3 == 0 && s.preparePerformance(to.string())) { ++prepared; serve(int(rng() % 20)); }
                s.loadPerformance(to.string());
                ++opened;
            }
            catch(const std::exception& e) { ++refused; if(std::getenv("OSCAIRN_STRESS_VERBOSE")) std::fprintf(stderr, "refused: %s\n", e.what()); }
            if(rng() % 4 == 0) { s.note(true, int(rng() % 128), .9); }
            serve(int(rng() % 40)); // the next change often comes while this one still rings out
        }
        s.setBudget({});
        serve(200);
        stop = true;
        audio.join();
        killer.join();
        render(s, 4);
        check(opened > switches / 4, "most changes opened: " + std::to_string(opened) + " of " + std::to_string(switches));
        check(rendered > 0, "the audio thread rendered");
        std::printf("%d switches: %d opened (%d prepared ahead), %d refused, %llu blocks, %d workers killed\n", switches, opened, prepared, refused, (unsigned long long)rendered.load(), killed.load());
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures) return 1;
    std::printf("ok: Performances switched under a rendering audio thread\n");
    return 0;
}
