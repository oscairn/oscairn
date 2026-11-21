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
// OSCAIRN_STRESS=N switches N times as often. First, the ring-out race (OSCAIRN_RINGOUT_RUNS, OSCAIRN_RINGOUT_PACE_US; see there).
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
#include <map>
#include <random>
#include <string>
#include <thread>
#include <vector>
#ifndef _WIN32
#include <sys/wait.h>
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

    // The unit's ST Final -> Into Yesterday 2 and # Organ -> Nameless Piano, in this repository's plugins (2026-11-21).
    // big: four Basic Synth parts alone on their engines (read, they share one instance, as ST Final's five Sampler parts
    // do), the first through a Delay, the last sending to a Reverb bus (a Delay); a plain instrument through the fixture's
    // effect; and the converter's empty bus, muted. organ: two Basic Synth parts (they join a playing engine), each through
    // the fixture's effect and a Delay, a Variation bus (a Delay), and the empty bus the parts send to at 0.0003 and 0.00025.
    // piano: one Basic Synth part and two empty buses.
    // OSCAIRN_RINGOUT_SAMPLER (a build of the Sampler, org.oscairn.sampler) and OSCAIRN_RINGOUT_SAMPLES (a folder holding
    // sine.osp, a one-record pack of a sine): the shared parts play the Sampler, as on the unit, instead of the Basic Synth.
    const char* sharedPlugin(const char* synth) { const char* p = std::getenv("OSCAIRN_RINGOUT_SAMPLER"); return p && *p ? p : synth; }
    void sharedSound(Session& s, NodeId n, const std::string& name, double brightness, int variant)
    {
        const char* samples = std::getenv("OSCAIRN_RINGOUT_SAMPLES");
        if(!std::getenv("OSCAIRN_RINGOUT_SAMPLER") || !samples) { check(s.selectSound(n, s.prepareSound(n, name, {{1, brightness}, {4, double(variant % 3)}})), "select a sound"); return; }
        const auto folder = s.registerAssetFolder(samples);
        const std::string sound = "oscairn-sampler-sound/0\nsample sine.osp\nelement ampeg_decay1=127 ampeg_decay2=127 ampeg_release=" + std::to_string(60 + 5 * variant) +
            "\nregion key=0-127 root=" + std::to_string(50 + variant) + " element=0 sample=0 loop=0-44100\n";
        check(s.selectSound(n, s.prepareSound(n, name, {}, {}, {}, sound, folder.library, folder.directory)), "select a Sampler sound");
    }
    void bigShape(const fs::path& file, const char* synth, const char* delay, const char* instrument)
    {
        Session s(48000, frames); synth = sharedPlugin(synth);
        const auto reverb = s.addPart("Reverb", {PartInput::Kind::Bus}); const auto rfx = s.add("reverb", delay); s.insertIntoPart(reverb, 0, rfx);
        const auto empty = s.addPart("Variation (type 03 00, not built yet)", {PartInput::Kind::Bus}); s.setPartLevel(empty, .57, true);
        std::vector<NodeId> synths;
        for(uint32_t i = 0; i < 4; ++i)
        {
            const auto n = s.add("pad " + std::to_string(i), synth); const auto part = s.addPart("Pad " + std::to_string(i)); s.insertIntoPart(part, 0, n);
            if(i == 0) { const auto fx = s.add("pad delay", delay); s.insertIntoPart(part, 1, fx); s.commit(); check(s.setParameter(fx, 1, .2) && s.setParameter(fx, 2, .4) && s.setParameter(fx, 3, .4), "delay settings"); }
            if(i == 3) s.setSend(part, reverb, .056);
            s.setPartLevel(part, .85, false); synths.push_back(n);
        }
        const auto n = s.add("keys", instrument), fx = s.add("keys effect", instrument, "org.kick.test.effect");
        const auto part = s.addPart("Keys"); s.insertIntoPart(part, 0, n); s.insertIntoPart(part, 1, fx);
        s.commit();
        check(s.setParameter(n, 7, .3) && s.setParameter(fx, 7, .8) && s.setParameter(rfx, 1, .3) && s.setParameter(rfx, 2, .5) && s.setParameter(rfx, 3, .5), "settings");
        for(size_t i = 0; i < synths.size(); ++i) sharedSound(s, synths[i], "Pad " + std::to_string(i), .3 + .1 * double(i), int(i));
        s.commit(); render(s, 2);
        s.savePerformance(file.string());
    }
    void organShape(const fs::path& file, const char* synth, const char* delay, const char* instrument)
    {
        Session s(48000, frames); synth = sharedPlugin(synth);
        const auto empty = s.addPart("Reverb (type 01 21, not built yet)", {PartInput::Kind::Bus}); s.setPartLevel(empty, .57, true);
        const auto variation = s.addPart("Variation", {PartInput::Kind::Bus}); const auto vfx = s.add("variation", delay); s.insertIntoPart(variation, 0, vfx);
        std::vector<NodeId> synths;
        for(uint32_t i = 0; i < 2; ++i)
        {
            const auto n = s.add("organ " + std::to_string(i), synth), a = s.add("rotary " + std::to_string(i), instrument, "org.kick.test.effect"), b = s.add("amp " + std::to_string(i), delay);
            const auto part = s.addPart("Organ " + std::to_string(i)); s.insertIntoPart(part, 0, n); s.insertIntoPart(part, 1, a); s.insertIntoPart(part, 2, b);
            s.commit(); s.setSend(part, empty, i ? .00025 : .0003); s.setPartLevel(part, i ? .62 : .86, false);
            check(s.setParameter(a, 7, .9) && s.setParameter(b, 1, .1) && s.setParameter(b, 2, .2) && s.setParameter(b, 3, .2), "organ effects");
            synths.push_back(n);
        }
        s.commit();
        check(s.setParameter(vfx, 1, .3) && s.setParameter(vfx, 2, .3) && s.setParameter(vfx, 3, .3), "variation settings");
        for(size_t i = 0; i < synths.size(); ++i) sharedSound(s, synths[i], "Organ " + std::to_string(i), .8, int(4 + i));
        s.setMaster(.497, 0); s.commit();
        render(s, 2);
        s.savePerformance(file.string());
    }
    void pianoShape(const fs::path& file, const char* synth)
    {
        Session s(48000, frames); synth = sharedPlugin(synth);
        const auto n = s.add("piano", synth); const auto part = s.addPart("Piano"); s.insertIntoPart(part, 0, n);
        for(const char* name : {"Reverb (type 01 02, not built yet)", "Variation (type 03 00, not built yet)"}) { const auto bus = s.addPart(name, {PartInput::Kind::Bus}); s.setPartLevel(bus, .25, true); }
        s.commit(); s.setPartLevel(part, .89, false); s.setMaster(.63, 0);
        sharedSound(s, n, "Piano", .5, 7);
        s.commit(); render(s, 2);
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

        // The unit's intermittent cut to digital zero (2026-11-21: ST Final -> Into Yesterday 2 cut 4 of 9 runs, # Organ ->
        // Nameless Piano 1 of 2), as the unit's script played it: one engine all along, each run opening its first
        // Performance (no note held), then a chord held by the pedal, then the change; the audio thread rendering on its
        // own, the control thread ticking every 2 to 25 ms, the change at any point of a tick. The chord must ring on: it
        // still sounds 0.8 s (300 blocks) after the change. The cause: the incoming parts joining the playing shared
        // engine had its whole state saved as the worker's recovery record, which excludes rendering; the block missed
        // so was reset for, which ended every note in it. With the blocks paced faster than a device's (300 us, so a save
        // overlaps one), it cut 5 of 60 changes in chain placement before the fix.
        // OSCAIRN_RINGOUT_RUNS: the runs (default 20), the cases in turn; OSCAIRN_RINGOUT_PACE_US: a block's pace.
        {
            const auto big = temp.path / "big", organ = temp.path / "organ", organ2 = temp.path / "organ-2", piano = temp.path / "piano";
            bigShape(big, argv[1], argv[2], argv[3]); organShape(organ, argv[1], argv[2], argv[3]); organShape(organ2, argv[1], argv[2], argv[3]); pianoShape(piano, argv[1]);
            const char* runsText = std::getenv("OSCAIRN_RINGOUT_RUNS");
            const char* paceText = std::getenv("OSCAIRN_RINGOUT_PACE_US");
            const int runs = runsText ? std::max(1, std::atoi(runsText)) : 20, pace = paceText ? std::max(50, std::atoi(paceText)) : 300;
            struct Case { const char* what; fs::path a, b; };
            const std::vector<Case> cases{{"ST Final -> Into Yesterday 2", big, organ}, {"# Organ -> Nameless Piano", organ2, piano}, {"Nameless Piano -> # Organ", piano, organ2},
                                          {"ST Final -> Nameless Piano", big, piano}, {"Nameless Piano -> ST Final", piano, big}};
            std::mt19937 rng(17);
            int cuts = 0;
            Session t(48000, frames);
            std::vector<float> peaks(1 << 16, 0.f); // each block's peak, by its number modulo the size
            std::atomic<size_t> block{0}; std::atomic<bool> done{false};
            std::thread audio([&] {
                std::array<float, frames> l{}, r{}; float* out[2]{l.data(), r.data()};
                auto next = std::chrono::steady_clock::now();
                while(!done)
                {
                    t.process(out, frames);
                    float p = 0; for(uint32_t f = 0; f < frames; ++f) p = std::max({p, std::abs(l[f]), std::abs(r[f])});
                    const auto b = block.load(std::memory_order_relaxed);
                    peaks[b % peaks.size()] = p;
                    block.store(b + 1, std::memory_order_release);
                    next += std::chrono::microseconds(pace); std::this_thread::sleep_until(next);
                }
            });
            struct Join { std::atomic<bool>& done; std::thread& audio; ~Join() { done = true; if(audio.joinable()) audio.join(); } } joined{done, audio};
            auto peak = [&](size_t from, size_t to) { float p = 0; for(size_t b = from; b < to; ++b) p = std::max(p, peaks[b % peaks.size()]); return p; };
#ifndef _WIN32
            // The record deferred: the workers killed while ST Final's chord still rings in the shared engine Into Yesterday 2's
            // parts joined restart with the engine's last record and those parts' snapshot prepared again: their sounds are
            // there, selected on their channels, and they play. Before the race, on a fresh engine; not in plugin hosts, whose restarts
            // within a minute the later kills here count.
            if(!placementPolicy().worker.empty() && !placementPolicy().hosts)
            {
                auto serve = [&](size_t untilBlock) { while(block.load(std::memory_order_acquire) < untilBlock) { t.service(); std::this_thread::sleep_for(std::chrono::milliseconds(10)); } };
                t.loadPerformance(big.string()); serve(block.load() + 100);
                t.controller(64, 127); t.note(true, 60, .8); serve(block.load() + 40); t.note(false, 60, 0);
                t.loadPerformance(organ.string()); serve(block.load() + 10);
                if(FILE* list = popen(("pgrep -P " + std::to_string(::getpid()) + " -f oscairn-plugin-worker").c_str(), "r"))
                {
                    char line[32];
                    while(std::fgets(line, sizeof line, list)) ::kill(std::atoi(line), SIGKILL);
                    pclose(list);
                }
                t.controller(64, 0);
                // Restarted: no failure listed, and each organ node's sounds readable (a worker still restarting refuses them); then theirs there, selected.
                std::string why; std::map<std::string, bool> there;
                for(int i = 0; i < 800; ++i)
                {
                    t.service(); std::this_thread::sleep_for(std::chrono::milliseconds(5));
                    const auto f = t.sessionStatus().failures; why = f.empty() ? "" : f[0].reason;
                    if(!why.empty()) continue;
                    try
                    {
                        there.clear();
                        for(const auto& [id, name] : t.graphState().names)
                            if(name.rfind("organ ", 0) == 0) { auto& ok = there[name]; for(const auto& c : t.sounds(id).contexts) ok = ok || (c.name == "Organ " + name.substr(6) && c.selectedChannels); }
                        break;
                    }
                    catch(const std::exception& e) { why = e.what(); }
                }
                check(why.empty() && there.size() == 2, "ring out race: the workers killed restart: " + why);
                for(const auto& [name, ok] : there) check(ok, "ring out race: after the restart " + name + "'s sound is there, selected");
                const size_t from = block.load();
                t.note(true, 60, .8); serve(block.load() + 200); t.note(false, 60, 0);
                check(peak(from + 100, from + 200) > 0, "ring out race: after the restart the new Performance plays");
                serve(block.load() + 100);
            }
#endif
            for(int run = 0; run < runs; ++run)
            {
                const auto& c = cases[size_t(run) % cases.size()];
                const int tickMs = 2 + int(rng() % 24);
                auto serve = [&](size_t untilBlock) { while(block.load(std::memory_order_acquire) < untilBlock) { t.service(); std::this_thread::sleep_for(std::chrono::milliseconds(tickMs)); } };
                t.setBudget(run % 3 ? Session::Budget{4, 0} : Session::Budget{}); // mostly with a budget: the estimate, the fit rule and measuring on, as on a device
                t.loadPerformance(c.a.string());
                serve(block.load() + 100);
                t.allNotesOff(); serve(block.load() + 40);
                t.controller(64, 127);
                for(const int k : {48, 60, 64, 67}) t.note(true, k, .8);
                serve(block.load() + 60);
                for(const int k : {48, 60, 64, 67}) t.note(false, k, 0);
                serve(block.load() + 40 + rng() % 8);
                std::this_thread::sleep_for(std::chrono::microseconds(rng() % (tickMs * 1000)));
                const size_t at = block.load(std::memory_order_acquire);
                std::string refused;
                try { t.loadPerformance(c.b.string()); } catch(const std::exception& e) { refused = e.what(); }
                const size_t loaded = block.load(std::memory_order_acquire);
                serve(loaded + 375);
                check(refused.empty(), std::string(c.what) + ": the change opens: " + refused);
                const float before = peak(at - 20, at), after = peak(loaded + 300, loaded + 375);
                size_t silent = 0; for(size_t b = at; b < loaded + 375; ++b) silent += peaks[b % peaks.size()] == 0;
                const bool cut = before == 0 || after == 0;
                cuts += cut;
                if(cut || std::getenv("OSCAIRN_STRESS_VERBOSE"))
                {
                    std::string notices; for(const auto& n : t.sessionStatus().notices) notices += n + "; ";
                    std::fprintf(stderr, "%s, run %d (tick %d ms, budget %d): level before %.5f, 300 blocks after the change %.5f; %zu silent blocks from the change (it took %zu); notices: %s\n",
                                 c.what, run, tickMs, run % 3 != 0, before, after, silent, loaded - at, notices.c_str());
                }
                t.controller(64, 0);
                serve(block.load() + 20);
            }
            std::printf("ring out race: %d of %d changes cut a pedaled chord\n", cuts, runs);
            check(cuts == 0, "ring out race: a pedaled chord rings on through every change: " + std::to_string(cuts) + " of " + std::to_string(runs) + " cut");
        }

#ifndef _WIN32
        // A Performance whose workers were given up (on the unit: failed as too slow, again and
        // again): the next one opens on instances of its own, not into the failed shared engines and
        // effect engines, which refused every later Performance using those plugins ("cannot restore
        // ... not restarted").
        // Every worker killed at the moment of a switch, before a supervisor's tick (every 10 ms) has seen it: the next
        // Performance opens all the same, its plugins loaded afresh where it would have taken over the dead instances,
        // with the document's state, it sounds, and a notice says which had stopped. Before, the restore into a dead
        // instance threw "cannot restore ...: Plugin worker connection lost" and the switch failed.
        if(!placementPolicy().worker.empty())
            for(int round = 0; round < 3; ++round)
            {
                Session g(48000, frames);
                g.loadPerformance(performances[0].string());
                render(g, 8);
                if(FILE* list = popen(("pgrep -P " + std::to_string(::getpid()) + " -f oscairn-plugin-worker").c_str(), "r"))
                {
                    std::vector<int> pids; char line[32];
                    while(std::fgets(line, sizeof line, list)) pids.push_back(std::atoi(line));
                    pclose(list);
                    for(const int pid : pids) ::kill(pid, SIGKILL);
                    for(const int pid : pids) // dead, not yet reaped (WNOWAIT: its supervisor reaps it)
                        for(siginfo_t info{}; ::waitid(P_PID, id_t(pid), &info, WEXITED | WNOHANG | WNOWAIT) == 0 && info.si_pid != pid; info = {}) std::this_thread::yield();
                }
                std::string refused;
                try { g.loadPerformance(performances[1].string()); } catch(const std::exception& e) { refused = e.what(); }
                check(refused.empty(), "a switch at a worker's death opens: " + refused);
                if(!refused.empty()) break;
                // The document's state: each delay's time and each plain instrument's gain (save's values for variant 1).
                bool state = true; std::string names;
                for(const auto& [id, name] : g.graphState().names)
                {
                    if(name.rfind("delay 1-", 0) == 0) state = state && std::abs(g.parameter(id, 1) - .13) < 1e-6;
                    if(name == "instrument 1-1" || name == "instrument 1-3") state = state && std::abs(g.parameter(id, 7) - .4) < 1e-6;
                    names += name + " ";
                }
                check(state, "its plugins hold the document's state: " + names);
                bool notice = false; for(const auto& n : g.sessionStatus().notices) notice = notice || n.find("had stopped") != std::string::npos;
                check(notice, "a notice says the plugins had stopped");
                g.note(true, 40, .9);
                std::array<float, frames> l{}, r{}; float* out[2]{l.data(), r.data()}; float peak = 0;
                for(int i = 0; i < 400 && peak < 1e-4f; ++i) { g.service(); g.process(out, frames); for(const float x : l) peak = std::max(peak, std::abs(x)); if(peak < 1e-4f) std::this_thread::sleep_for(std::chrono::milliseconds(2)); }
                check(peak >= 1e-4f, "and it sounds: peak " + std::to_string(peak));
            }
        // Every worker STOPPED at the moment of a switch, alive and not answering: on the unit, a worker killed the instant
        // the next Performance was asked for was still dying, not yet seen as exited, when the open reached it. The restore
        // into its instance gets no reply in the reply timeout (round 0), or its connection drops as it is then killed while
        // the open waits on it (round 1). The open fails that worker as its supervisor would, loads its plugins afresh with
        // the document's state, and opens, with the notice, and sounds. Before, it was refused ("connection lost").
        if(!placementPolicy().worker.empty())
            for(int round = 0; round < 2; ++round)
            {
                // Not round 0 with plugin hosts: a fresh load picked into a stopped host waits out a load's full 30 s (no audio
                // runs here to fail it for blocks missed), which is how any load into a hung host fares, not this case.
                if(round == 0 && std::getenv("OSCAIRN_PLUGIN_HOSTS")) continue;
                ::setenv("OSCAIRN_WORKER_REPLY_SECONDS", round == 0 ? "1" : "30", 1); // read as each worker process starts
                Session g(48000, frames);
                g.loadPerformance(performances[0].string());
                render(g, 8);
                std::vector<int> pids;
                if(FILE* list = popen(("pgrep -P " + std::to_string(::getpid()) + " -f oscairn-plugin-worker").c_str(), "r"))
                {
                    char line[32];
                    while(std::fgets(line, sizeof line, list)) pids.push_back(std::atoi(line));
                    pclose(list);
                }
                check(!pids.empty(), "the Performance plays in workers");
                for(const int pid : pids) ::kill(pid, SIGSTOP);
                std::thread killer; // round 1: killed while the open waits on them (or, should the open reach none in time, just before)
                if(round == 1) killer = std::thread([&] { std::this_thread::sleep_for(std::chrono::milliseconds(300)); for(const int pid : pids) ::kill(pid, SIGKILL); });
                std::string refused;
                try { g.loadPerformance(performances[1].string()); } catch(const std::exception& e) { refused = e.what(); }
                if(killer.joinable()) killer.join();
                for(const int pid : pids) ::kill(pid, SIGCONT); // those the open did not fail answer again
                check(refused.empty(), "a switch at a worker stopped (round " + std::to_string(round) + ") opens: " + refused);
                if(!refused.empty()) break;
                bool state = true; std::string names;
                for(const auto& [id, name] : g.graphState().names)
                {
                    if(name.rfind("delay 1-", 0) == 0) state = state && std::abs(g.parameter(id, 1) - .13) < 1e-6;
                    if(name == "instrument 1-1" || name == "instrument 1-3") state = state && std::abs(g.parameter(id, 7) - .4) < 1e-6;
                    names += name + " ";
                }
                check(state, "its plugins hold the document's state: " + names);
                bool notice = false; for(const auto& n : g.sessionStatus().notices) notice = notice || n.find("had stopped") != std::string::npos;
                check(notice, "a notice says the plugins had stopped");
                g.note(true, 40, .9);
                std::array<float, frames> l{}, r{}; float* out[2]{l.data(), r.data()}; float peak = 0;
                for(int i = 0; i < 400 && peak < 1e-4f; ++i) { g.service(); g.process(out, frames); for(const float x : l) peak = std::max(peak, std::abs(x)); if(peak < 1e-4f) std::this_thread::sleep_for(std::chrono::milliseconds(2)); }
                check(peak >= 1e-4f, "and it sounds: peak " + std::to_string(peak));
            }
        ::unsetenv("OSCAIRN_WORKER_REPLY_SECONDS");
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
