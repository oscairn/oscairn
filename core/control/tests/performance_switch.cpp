// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Switching Performances (RFC 0021, "Fewer instances: reusing what is loaded"): how long the
// engine takes from performance.load until the next one plays, how many plugin instances it
// loads and how many it takes over, between two Performances that use the same plugins and two
// that share none; and that a reused instance sounds exactly as a fresh load of that state.
// Each shared Performance has four parts: two Basic Synths (native sound contexts) and two plain
// CLAP instruments (the prepared-instance pool), each through a Delay instance of its own. The
// other has four Samplers.
//   oscairn-performance-switch-test SYNTH.clap SAMPLER.clap DELAY.clap INSTRUMENT.clap
// With OSCAIRN_SWITCH_PERFORMANCES="A|B|C" (folders of saved Performances, A and B sharing
// plugins, C not), the same switches are timed with those too.
#include "oscairn/session.h"
#include "oscairn/plugin_host.h"
#include "oscairn/wav_file.h"
#include "AllocationGuard.h"
#include "../../plugin-host/src/StateFiles.h"
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <thread>
#if defined(__unix__) || defined(__APPLE__)
#include <unistd.h>
#endif
#ifdef __APPLE__
#include <libproc.h>
#include <mach/mach_time.h>
#endif
#include "../../test-support/relaxed.h"
using oscairn::test::relaxed;

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
        bool service = true; // false: a block without the control thread's tick (it ticks every 20 ms on a device)
        bool sounding() const { return std::any_of(l.begin(), l.end(), [](float x) { return x != 0; }); }
        void run(Session& s)
        {
            if(service) s.service();
            forbidAllocation = true;
            const bool ok = s.process(out, frames);
            forbidAllocation = false; check(ok, "render");
        }
    };
    void blocks(Session& s, int n) { Audio a; for(int i = 0; i < n; ++i) a.run(s); }
    // Long enough for every plugin to have been silent the three seconds ringing out asks.
    void quiet(Session& s) { blocks(s, int(48000 * 3.2 / frames)); }

    void shared(const fs::path& file, int variant, const char* synth, const char* delay, const char* instrument)
    {
        Session s(48000, frames);
        for(uint32_t i = 0; i < 4; ++i)
        {
            const auto name = std::to_string(variant) + "-" + std::to_string(i);
            const auto n = s.add("instrument " + name, i % 2 ? instrument : synth), fx = s.add("delay " + name, delay);
            PartInput keys; keys.zones = {NoteZone{uint8_t(32 * i), uint8_t(32 * i + 31)}}; // a quarter of the keyboard each
            const auto part = s.addPart("Part " + name, keys);
            s.insertIntoPart(part, 0, n); s.insertIntoPart(part, 1, fx); s.commit();
            if(i % 2) check(s.setParameter(n, 7, .3 + .2 * variant + .05 * i), "instrument gain");
            else check(s.selectSound(n, s.prepareSound(n, "Saw " + name, {{4, double(1 + variant)}, {5, double(3 + i)}})), "select a sound");
            check(s.setParameter(fx, 1, .1 + .03 * variant + .02 * i) && s.setParameter(fx, 2, .3 + .1 * variant), "delay settings");
        }
        blocks(s, 2);
        s.savePerformance(file.string());
    }
    void other(const fs::path& file, const char* sampler, const fs::path& samples)
    {
        Session s(48000, frames);
        const auto folder = s.registerAssetFolder(samples.string());
        for(uint32_t i = 0; i < 4; ++i)
        {
            const auto n = s.add("sampler " + std::to_string(i), sampler);
            const auto part = s.addPart("Part " + std::to_string(i));
            s.insertIntoPart(part, 0, n); s.commit();
            check(s.selectSound(n, s.prepareSound(n, "Sample", {}, {}, {}, "<region> sample=sample.wav ampeg_release=0.2", folder.library, folder.directory)), "sampler sound");
        }
        blocks(s, 2);
        s.savePerformance(file.string());
    }

    // `count` plain CLAP instruments, a part of three keys each.
    void many(const fs::path& file, int variant, const char* instrument, uint32_t count)
    {
        Session s(48000, frames);
        for(uint32_t i = 0; i < count; ++i)
        {
            const auto n = s.add("instrument " + std::to_string(i), instrument);
            PartInput keys; keys.zones = {NoteZone{uint8_t(3 * i), uint8_t(3 * i + 2)}};
            const auto part = s.addPart("Part " + std::to_string(i), keys);
            s.insertIntoPart(part, 0, n); s.commit();
            check(s.setParameter(n, 7, .2 + .3 * variant + .01 * i), "instrument gain");
        }
        blocks(s, 2);
        s.savePerformance(file.string());
    }

    // One plain CLAP instrument on the whole keyboard at `gain`, its part at `level` and the master at `master`.
    void single(const fs::path& file, const char* instrument, double gain, double level = 1, double master = 1)
    {
        Session s(48000, frames);
        const auto n = s.add("instrument", instrument);
        const auto part = s.addPart("Part"); s.insertIntoPart(part, 0, n); s.commit();
        check(s.setParameter(n, 7, gain), "instrument gain");
        s.setPartLevel(part, level, false); s.setMaster(master, 0);
        blocks(s, 2);
        s.savePerformance(file.string());
    }

    // Two Basic Synth parts, a quarter of the keyboard each, each playing the last of `sounds`
    // prepared sounds (native contexts beside its Default).
    void synths(const fs::path& file, int variant, const char* synth, int sounds = 1)
    {
        Session s(48000, frames);
        for(uint32_t i = 0; i < 2; ++i)
        {
            const auto name = std::to_string(variant) + "-" + std::to_string(i);
            const auto n = s.add("synth " + name, synth);
            PartInput keys; keys.zones = {NoteZone{uint8_t(32 * i), uint8_t(32 * i + 31)}};
            const auto part = s.addPart("Part " + name, keys);
            s.insertIntoPart(part, 0, n); s.commit();
            uint32_t id = 0;
            for(int k = 0; k < sounds; ++k) id = s.prepareSound(n, "Sound " + name + "-" + std::to_string(k), {{1, .3 + .2 * variant}, {4, double((variant + i + k) % 3)}, {5, double(3 + i + 2 * variant + k)}});
            check(s.selectSound(n, id), "select a sound");
        }
        blocks(s, 2);
        s.savePerformance(file.string());
    }
    // `count` Basic Synth parts, ten keys each, each playing its own values on its Default, saved with
    // an instance each (each instance full when the next was added, then emptied): read, they share.
    void crowd(const fs::path& file, const char* synth, uint32_t count)
    {
        Session s(48000, frames);
        std::vector<NodeId> nodes;
        for(uint32_t i = 0; i < count; ++i)
        {
            const auto n = s.add("crowd " + std::to_string(i), synth);
            PartInput keys; keys.zones = {NoteZone{uint8_t(10 * i), uint8_t(10 * i + 9)}};
            const auto part = s.addPart("Part " + std::to_string(i), keys);
            s.insertIntoPart(part, 0, n); s.commit();
            check(s.setParameter(n, 1, .2 + .05 * i) && s.setParameter(n, 5, double(i)), "crowd values");
            for(int k = 0; k < 7; ++k) s.prepareSound(n, "filler", {});
            nodes.push_back(n);
        }
        blocks(s, 2);
        for(const auto n : nodes) for(const auto& c : s.sounds(n).contexts) if(c.name == "filler") s.discardSound(n, c.id);
        blocks(s, 2);
        s.savePerformance(file.string());
    }
    // One Basic Synth part on the whole keyboard playing one sound: with `delay`, through a Delay
    // (a quarter beat, feedback and wet at half and more), at the part's `level`; `brightness`, or
    // one from the variant.
    void ringing(const fs::path& file, int variant, const char* synth, const char* delay, double level, double brightness = -1)
    {
        Session s(48000, frames);
        const auto n = s.add("synth", synth);
        const auto part = s.addPart("Part"); s.insertIntoPart(part, 0, n);
        if(delay)
        {
            const auto fx = s.add("delay", delay); s.insertIntoPart(part, 1, fx); s.commit();
            check(s.setParameter(fx, 1, .25) && s.setParameter(fx, 2, .5) && s.setParameter(fx, 3, .6), "delay settings");
        }
        s.commit(); s.setPartLevel(part, level, false);
        check(s.selectSound(n, s.prepareSound(n, "Ring " + std::to_string(variant), {{1, brightness < 0 ? .4 + .3 * variant : brightness}, {4, double(variant % 3)}})), "select a sound");
        blocks(s, 2);
        s.savePerformance(file.string());
    }
    // The converter writes the send wire to its muted empty bus unmuted, at level 0: the saved Performance made so.
    void unmuteEmptySend(const fs::path& file)
    {
        auto bytes = state_files::read(file / "performance.json", 4 << 20); std::string text(bytes.begin(), bytes.end());
        const std::string muted = "\"level\":0,\"pan\":0,\"mute\":true";
        const auto at = text.find(muted); check(at != std::string::npos, "the send wire to the empty bus");
        if(at != std::string::npos) text.replace(at, muted.size(), "\"level\":0,\"pan\":0,\"mute\":false");
        state_files::write(file / "performance.json", text.data(), text.size());
    }
    // An effect for chained: the Delay (by path) or the fixture's effect (path and plugin ID).
    struct Effect { const char* path = nullptr; const char* id = nullptr; };
    // One Basic Synth part on the whole keyboard playing one sound marked by `brightness`: through
    // `insert` when given, and sending to a bus through `send` when given; `setting` sets the
    // effects (the Delay's feedback and wet, the fixture's gain).
    // With `pair`, two such parts, the keyboard's lower and upper halves, the second through an
    // insert alone: read, the two synths share one instance (formed on their own).
    // With `empty`, the first part also sends at level 0 to an empty bus, muted at a quarter level, as the
    // converter writes a bus it did not build ("Reverb (type 01 02, not built yet)"): a level-0 wire
    // from the part straight to the outputs.
    void chained(const fs::path& file, const char* synth, Effect insert, Effect send, double brightness, double setting, bool pair = false, bool empty = false)
    {
        Session s(48000, frames);
        auto effect = [&](const std::string& name, Effect e) {
            const auto fx = e.id ? s.add(name, e.path, e.id) : s.add(name, e.path);
            if(e.id) check(s.setParameter(fx, 7, setting), "effect gain");
            else check(s.setParameter(fx, 1, .25) && s.setParameter(fx, 2, setting) && s.setParameter(fx, 3, setting), "delay settings");
            return fx;
        };
        std::vector<NodeId> synths;
        for(int i = 0; i < (pair ? 2 : 1); ++i)
        {
            const auto n = s.add("synth " + std::to_string(i), synth);
            PartInput keys; if(pair) keys.zones = {NoteZone{uint8_t(64 * i), uint8_t(64 * i + 63)}};
            const auto part = s.addPart("Part " + std::to_string(i), keys); s.insertIntoPart(part, 0, n);
            if(insert.path) s.insertIntoPart(part, 1, effect("insert " + std::to_string(i), insert));
            if(send.path && !i)
            {
                const auto bus = s.addPart("Bus", {PartInput::Kind::Bus}); s.insertIntoPart(bus, 0, effect("send", send));
                s.setSend(part, bus, .5);
            }
            if(empty && !i)
            {
                const auto bus = s.addPart("Reverb (not built yet)", {PartInput::Kind::Bus});
                s.setPartLevel(bus, .25, true); s.setSend(part, bus, 0);
            }
            synths.push_back(n);
        }
        s.commit();
        for(size_t i = 0; i < synths.size(); ++i)
            check(s.selectSound(synths[i], s.prepareSound(synths[i], "Chained " + std::to_string(brightness) + " " + std::to_string(i), {{1, brightness}, {4, double(1 + i)}})), "select a sound");
        blocks(s, 2);
        s.savePerformance(file.string());
        if(empty) unmuteEmptySend(file);
    }
    // One Sampler part on the whole keyboard, a 4 s sample (long.wav) through a Delay (as the owner's sampled Performances: a
    // Sampler, then effects); with `empty`, also the converter's level-0 send to an empty bus (chained's `empty`).
    void sampled(const fs::path& file, const char* sampler, const char* delay, const fs::path& samples, bool empty)
    {
        Session s(48000, frames);
        const auto folder = s.registerAssetFolder(samples.string());
        const auto n = s.add("sampler", sampler), fx = s.add("delay", delay);
        const auto part = s.addPart("Piano"); s.insertIntoPart(part, 0, n); s.insertIntoPart(part, 1, fx);
        if(empty) { const auto bus = s.addPart("Reverb (not built yet)", {PartInput::Kind::Bus}); s.setPartLevel(bus, .25, true); s.setSend(part, bus, 0); }
        s.commit();
        check(s.setParameter(fx, 1, .25) && s.setParameter(fx, 2, .3) && s.setParameter(fx, 3, .3), "delay settings");
        check(s.selectSound(n, s.prepareSound(n, "Looped", {}, {}, {}, "<region> sample=long.wav ampeg_release=0.5", folder.library, folder.directory)), "sampler sound");
        blocks(s, 2);
        s.savePerformance(file.string());
        if(empty) unmuteEmptySend(file);
    }
    // One Basic Synth part on the keys up to 40 alone.
    void lower(const fs::path& file, const char* synth)
    {
        Session s(48000, frames);
        const auto n = s.add("synth", synth);
        PartInput keys; keys.zones = {NoteZone{0, 40}};
        const auto part = s.addPart("Saw", keys); s.insertIntoPart(part, 0, n); s.commit();
        check(s.selectSound(n, s.prepareSound(n, "Saw low", {{1, .5}})), "select a sound");
        blocks(s, 2);
        s.savePerformance(file.string());
    }
    // A CPU budget no change reaches and a memory budget whose ceiling is `bytes`: the ceiling is the budget less the worker
    // program's code where plugins run in workers (memoryCeiling; Linux counts it, macOS does not), read from the estimate.
    void allow(Session& s, uint64_t bytes)
    {
        const uint64_t big = uint64_t(1) << 40;
        s.setBudget({64, big});
        s.setBudget({64, bytes + (big - s.estimate("").budget.memory)});
    }
    struct Switch { double ms = 0; uint64_t loaded = 0, reused = 0, kept = 0, beside = 0; Session::Timing::Open open; };
    Switch load(Session& s, const fs::path& to)
    {
        Switch w;
        const auto loaded = pluginInstancesLoaded(), reused = s.reusedInstances(), kept = s.keptStates(), beside = s.besideSounds();
        const auto start = std::chrono::steady_clock::now();
        s.loadPerformance(to.string());
        w.ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start).count();
        w.loaded = pluginInstancesLoaded() - loaded; w.reused = s.reusedInstances() - reused; w.kept = s.keptStates() - kept; w.beside = s.besideSounds() - beside;
        // engine.timing's open says the same, and its stages fit in its read and its whole.
        w.open = s.engineTiming(false).open;
        const auto& o = w.open;
        check(o.reused == w.reused && o.kept == w.kept && o.loaded == w.loaded, "engine.timing's open counts what the change did: " + std::to_string(o.reused) + " reused, " + std::to_string(o.loaded) + " loaded");
        check(o.total > 0 && o.total <= w.ms + 1 && o.publish <= o.total && o.load + o.restore + o.reset + o.save <= o.read + 1e-3, "engine.timing's open times add up");
        return w;
    }
    void report(const char* what, const Switch& w)
    {
        std::printf("%-52s %7.1f ms, %2llu instances loaded, %2llu reused (%llu holding their state, %llu with sounds beside)\n", what, w.ms, (unsigned long long)w.loaded, (unsigned long long)w.reused, (unsigned long long)w.kept, (unsigned long long)w.beside);
        std::printf("%-52s read %.1f ms (loads %.1f, state loads %.1f, resets %.1f, state saves %.1f), publish %.1f ms\n", "", w.open.read, w.open.load, w.open.restore, w.open.reset, w.open.save, w.open.publish);
    }

    // The same notes into a Session that switched and one that opened `file` fresh: every sample equal.
    // The first notes at block `start`; 0 plays one in the block the change is heard in.
    void compare(Session& switched, const fs::path& file, const char* what, int start = 3)
    {
        Session fresh(48000, frames);
        fresh.loadPerformance(file.string());
        Audio x, y; double energy = 0; bool same = true;
        for(int block = 0; block < 600 && same; ++block)
        {
            // A note for each part: they have a quarter of the keyboard each.
            for(const int key : {20, 60, 70, 100})
            {
                if(block == start + key % 7) { switched.note(true, key, .8); fresh.note(true, key, .8); }
                if(block == 150 + key % 5) { switched.note(false, key, 0); fresh.note(false, key, 0); }
            }
            x.run(switched); y.run(fresh);
            if(x.l != y.l || x.r != y.r) { std::fprintf(stderr, "  different at block %d\n", block); same = false; }
            for(auto v : x.l) energy += v * v;
        }
        check(same, std::string(what) + ": a reused instance sounds as a fresh load of that state");
        check(energy > 1e-3, std::string(what) + ": audible");
    }

    // Resident memory of this process and its plugin workers, in MiB, from ps; 0 where there is none.
    double resident()
    {
#if defined(__unix__) || defined(__APPLE__)
        std::map<long, std::pair<long, long>> all; // pid: parent, KiB
        if(FILE* f = popen("ps -A -o pid= -o ppid= -o rss=", "r"))
        {
            long pid, parent, kib;
            while(std::fscanf(f, "%ld %ld %ld", &pid, &parent, &kib) == 3) all[pid] = {parent, kib};
            pclose(f);
        }
        auto ours = [&](long pid) { for(int depth = 0; pid > 1 && depth < 8; ++depth) { if(pid == long(getpid())) return true; const auto it = all.find(pid); if(it == all.end()) return false; pid = it->second.first; } return false; };
        long kib = 0;
        for(const auto& [pid, x] : all) if(ours(pid)) kib += x.second;
        return kib / 1024.0;
#else
        return 0;
#endif
    }
    // This process's child processes now: its plugin workers.
    size_t children()
    {
        size_t n = 0;
#if defined(__unix__) || defined(__APPLE__)
        if(FILE* f = popen("ps -A -o ppid=", "r"))
        {
            long parent;
            while(std::fscanf(f, "%ld", &parent) == 1) n += parent == long(getpid());
            pclose(f);
        }
#endif
        return n;
    }
    // A process's CPU time so far, in seconds; -1 where it cannot be read.
    double cpuOf(long pid)
    {
#ifdef __linux__
        std::ifstream stat("/proc/" + std::to_string(pid) + "/stat"); std::string text; std::getline(stat, text);
        const auto close = text.rfind(')'); if(close == std::string::npos) return -1;
        std::istringstream rest(text.substr(close + 2)); std::string field; unsigned long long user = 0, system = 0;
        for(int i = 3; i <= 15 && rest >> field; ++i) { if(i == 14) user = std::stoull(field); if(i == 15) system = std::stoull(field); }
        return double(user + system) / double(sysconf(_SC_CLK_TCK));
#elif defined(__APPLE__)
        proc_taskinfo info{};
        if(proc_pidinfo(int(pid), PROC_PIDTASKINFO, 0, &info, sizeof(info)) != int(sizeof(info))) return -1;
        mach_timebase_info_data_t base{}; mach_timebase_info(&base);
        return double(info.pti_total_user + info.pti_total_system) * base.numer / base.denom / 1e9;
#else
        (void)pid; return -1;
#endif
    }
    void memory(const char* what, Session& s) { std::printf("%-52s %7.0f MiB resident, %zu instances kept warm\n", what, resident(), s.warmInstances()); }

    void timeOnly(Session& s, const std::vector<fs::path>& p)
    {
        // Real Performances (OSCAIRN_SWITCH_PERFORMANCES): A cold, then A and B (sharing plugins) in
        // turn five times, each silent before the change, then C (sharing little) and A again.
        report("real: open A (cold)", load(s, p[0])); quiet(s);
        memory("real: A open", s);
        std::vector<Switch> ab, ba;
        for(int round = 0; round < 5; ++round) { ab.push_back(load(s, p[1])); quiet(s); ba.push_back(load(s, p[0])); quiet(s); }
        memory("real: A open after switching with B", s);
        auto median = [](std::vector<Switch> v) { std::sort(v.begin(), v.end(), [](const Switch& x, const Switch& y) { return x.ms < y.ms; }); return v[v.size() / 2]; };
        report("real: A -> B, the first", ab[0]);
        report("real: B -> A, the first", ba[0]);
        report("real: A -> B, A silent (median of the other 4)", median({ab.begin() + 1, ab.end()}));
        report("real: B -> A, B silent (median of the other 4)", median({ba.begin() + 1, ba.end()}));
        report("real: A -> C (shares little)", load(s, p[2]));
        for(int i = 0; i < int(48000 * 31 / frames) && s.leaving(); ++i) blocks(s, 1);
        report("real: C -> A (A's instruments kept warm)", load(s, p[0]));
    }
    // Issue 330's steps (control-performance-switch-steps, OSCAIRN_SWITCH_STEPS): on their own, the rest of this one is long enough.
    void issue330(char** argv, const fs::path& tempPath, const fs::path& samples)
    {
        // With a build of the Sampler (OSCAIRN_SWITCH_SAMPLER) and a folder holding sine.osp (OSCAIRN_SWITCH_SAMPLES): FANTOM k6's
        // "again" series as on the unit, five Sampler parts each through a Delay; A, A four more times, a synth through a Delay,
        // then A: the return takes A's instances kept warm back holding A's state, as many as a series without the repeats.
        if(const char* sampler = std::getenv("OSCAIRN_SWITCH_SAMPLER"); sampler && *sampler)
        {
            const char* pack = std::getenv("OSCAIRN_SWITCH_SAMPLES");
            if(!pack || !*pack) throw std::runtime_error("OSCAIRN_SWITCH_SAMPLES names the folder holding sine.osp");
            const auto a = tempPath / "real-a", organ = tempPath / "real-organ";
            ::unsetenv("OSCAIRN_SHARING"); // parts apart, as on the unit, saved so too
            {
                Session s(48000, frames);
                const auto folder = s.registerAssetFolder(pack);
                for(int i = 0; i < 5; ++i)
                {
                    const auto n = s.add("sampler " + std::to_string(i), sampler), fx = s.add("delay " + std::to_string(i), argv[3]);
                    const auto part = s.addPart("Layer " + std::to_string(i)); s.insertIntoPart(part, 0, n); s.insertIntoPart(part, 1, fx); s.commit();
                    check(s.setParameter(fx, 1, .2 + .02 * i) && s.setParameter(fx, 2, .3) && s.setParameter(fx, 3, .3), "delay settings");
                    const std::string sound = "oscairn-sampler-sound/0\nsample sine.osp\nelement ampeg_decay1=127 ampeg_decay2=127 ampeg_release=" + std::to_string(40 + 5 * i) +
                        "\nregion key=0-127 root=" + std::to_string(48 + i) + " element=0 sample=0 loop=0-44100\n";
                    check(s.selectSound(n, s.prepareSound(n, "Layer " + std::to_string(i), {}, {}, {}, sound, folder.library, folder.directory)), "a Sampler sound");
                }
                blocks(s, 2); s.savePerformance(a.string());
            }
            // The other: a synth through a Delay, and one Sampler part of a sound of its own (# Organ has one).
            {
                Session s(48000, frames);
                const auto folder = s.registerAssetFolder(pack);
                const auto n = s.add("organ sampler", sampler), fx = s.add("organ delay", argv[3]), synth = s.add("organ synth", argv[1]);
                const auto part = s.addPart("Organ"); s.insertIntoPart(part, 0, n); s.insertIntoPart(part, 1, fx);
                const auto lead = s.addPart("Lead"); s.insertIntoPart(lead, 0, synth); s.commit();
                const std::string sound = "oscairn-sampler-sound/0\nsample sine.osp\nelement ampeg_release=90\nregion key=0-127 root=60 element=0 sample=0 loop=0-44100\n";
                check(s.selectSound(n, s.prepareSound(n, "Organ", {}, {}, {}, sound, folder.library, folder.directory)), "the other's Sampler sound");
                blocks(s, 2); s.savePerformance(organ.string());
            }
            auto series = [&](int repeats) {
                Session t(48000, frames);
                load(t, a); quiet(t);
                for(int i = 0; i < repeats; ++i) { const auto w = load(t, a); report(("real: A again " + std::to_string(i + 1)).c_str(), w); quiet(t); }
                load(t, organ); quiet(t); quiet(t);
                const auto w = load(t, a); report(repeats ? "real: back to A after repeats" : "real: back to A", w);
                return w;
            };
            const auto plain = series(0), repeated = series(4);
            check(repeated.kept == plain.kept && repeated.loaded == plain.loaded, "real Sampler: back to A after kept repeats keeps as many as with none: " + std::to_string(repeated.kept) + " against " + std::to_string(plain.kept));
            ::setenv("OSCAIRN_SHARING", "memory", 1);
        }
    // Issue 330, the owner's rule (2026-11-27): the new Performance loads; the last one's held notes ring on only as far as
    // that allows. The memory budget lets the incoming Performance fit alone and not beside the whole of the last, so before
    // these the change cut every held note (about 1 ms on the FANTOM). Each Session renders on a thread of its own, as on a
    // device, at about 2.7 times its pace, so a fade made in the load is heard in the blocks; the fixture instrument sounds
    // its gain as a constant while a key is down, so each part's level reads straight off the output.
    {
        struct Live
        {
            Session s{48000, frames};
            std::vector<float> left = std::vector<float>(size_t(frames) * 30000); // every block's left channel, in order
            std::atomic<size_t> block{0}; std::atomic<bool> done{false};
            std::thread audio;
            Live()
            {
                audio = std::thread([this] {
                    std::array<float, frames> l{}, r{}; float* out[2]{l.data(), r.data()};
                    auto next = std::chrono::steady_clock::now();
                    while(!done)
                    {
                        s.process(out, frames);
                        const auto b = block.load(std::memory_order_relaxed);
                        if((b + 1) * frames <= left.size()) std::copy(l.begin(), l.end(), left.begin() + std::ptrdiff_t(b * frames));
                        block.store(b + 1, std::memory_order_release);
                        next += std::chrono::microseconds(1000); std::this_thread::sleep_until(next);
                    }
                });
            }
            ~Live() { done = true; audio.join(); }
            void serve(size_t n) { const auto until = block.load() + n; while(block.load(std::memory_order_acquire) < until) { s.service(); std::this_thread::sleep_for(std::chrono::milliseconds(3)); } }
            float at(size_t b, size_t f = 0) const { return left[b * frames + f]; }
            bool said(const std::string& text) { for(const auto& n : s.sessionStatus().notices) if(n.find(text) != std::string::npos) return true; return false; }
            bool has(const std::string& name) { for(const auto& [id, n] : s.graphState().names) if(n == name) return true; return false; }
            uint64_t memoryOf(size_t i) { return s.estimate("").plugins.at(i).memory; }
        };
        // From the first block at or after `from` whose first sample is not `was`, to the first after it at `now` throughout:
        // how many samples lay between the two, the largest step from one sample to the next, and whether it reached `now`.
        struct Transition { size_t between = 0; float step = 0; bool reached = false; };
        auto transition = [](const Live& l, size_t from, size_t to, float was, float now) {
            Transition t; const float span = std::abs(was - now), lo = std::min(was, now) + .02f * span, hi = std::max(was, now) - .02f * span;
            float last = l.at(from);
            for(size_t i = from * frames; i < to * frames; ++i)
            {
                const float v = l.left[i];
                t.step = std::max(t.step, std::abs(v - last)); last = v;
                if(v > lo && v < hi) ++t.between;
                if(std::abs(v - now) < 1e-5f) t.reached = true; else if(t.reached && std::abs(v - now) >= 1e-5f) t.reached = false;
            }
            return t;
        };
        const auto incoming = tempPath / "steps-incoming"; ringing(incoming, 1, argv[1], argv[3], .5); // a synth through a Delay, nothing held in it

        // A silent instance holding the node's state is taken over as before issue 330: its state saved and compared with the file,
        // then reset (or given the state again where they differ), never kept unseen. FANTOM k8: a Sampler loaded fresh saves a state
        // that differs from its file until it is given it again; kept as it was through the direct repeats, it still differed when
        // its warm instance was taken back after another Performance, so the return loaded every state again (kept 0, where main
        // kept 3 to 4). Only a sounding instance, which cannot be saved without a reset in a worker, is kept unseen.
        {
            const auto same = tempPath / "steps-silent-same"; single(same, argv[4], .3);
            Session t(48000, frames);
            load(t, same); quiet(t);
            const auto w = load(t, same); report("steps: the same Performance again, silent", w);
            check(w.reused == 1 && w.loaded == 0 && w.open.save > 0 && t.sessionStatus().ringOut.kept == 0, "steps, silent: the instance taken over and compared with its file, not kept unseen: " +
                  std::to_string(w.open.save) + " ms saving, " + std::to_string(t.sessionStatus().ringOut.kept) + " kept unseen");
        }
        // Issue 9: nothing sounding, and the two Performances do not fit together (FANTOM d1: the engine said "opened at once ... would
        // not fit" at nearly every plain load and let every instance kept warm go, a VA the Performance after next takes among them,
        // which then loaded afresh in a second). Nothing heard, nothing given up: no cut, the last one's silent instances go as
        // the memory needs, and the instances kept warm stay as far as there is room for them. X's instrument kept warm,
        // A's synth and Delay playing, which the incoming takes over (as Hopeless takes TSO's); memory classes with room for the
        // incoming beside all but a byte of A. Before, the cut let X go, and so did the room made before the read, which counted
        // what the incoming takes over twice, as A's and again as its.
        {
            const auto x = tempPath / "steps-warm-x", c = tempPath / "steps-warm-a"; single(x, argv[4], .5); ringing(c, 0, argv[1], argv[3], .5);
            Session t(48000, frames);
            auto gone = [&] { for(int i = 0; i < int(48000 * 31 / frames) && t.leaving(); ++i) blocks(t, 1); };
            load(t, x); quiet(t); load(t, c); gone(); quiet(t);
            check(t.warmInstances() == 1, "steps, kept warm: X's instrument kept warm: " + std::to_string(t.warmInstances()));
            uint64_t xBytes = 0; for(const auto& i : t.engineTiming(false).instances) if(!i.inUse) xBytes = i.memory;
            Session::Budget b{64, 0, uint64_t(1) << 40, 0, 0, 0};
            t.setBudget(b); t.forgetMeasurements(); blocks(t, 2);
            const auto next = t.estimate(incoming.string()).memory, open = t.estimate("").memory;
            b.device = t.memoryState().reserved + next + open - 1;
            t.setBudget(b);
            // Room for X beside A: what the incoming's estimate is over a byte (a sanitizer's worker holds more than that guess: X goes here).
            const bool fitsBefore = xBytes < next;
            check(t.warmInstances() == (fitsBefore ? 1u : 0u) && !t.estimate(incoming.string()).seamless && t.estimate(incoming.string()).fits, "steps, kept warm: the incoming fits alone, not beside A, X still kept warm where it fits");
            const auto before = t.sessionStatus().ringOut;
            load(t, incoming);
            const auto r = t.sessionStatus().ringOut; const auto m = t.memoryState();
            const bool room = xBytes <= (m.device > m.reserved + m.committed ? m.device - m.reserved - m.committed : 0);
            std::printf("steps, kept warm: %llu let go silent, %zu kept warm (X %llu bytes, room %s)\n", (unsigned long long)(r.releasedSilent - before.releasedSilent), t.warmInstances(), (unsigned long long)xBytes, room ? "for it" : "short");
            check(r.cut == before.cut && r.releasedSilent > before.releasedSilent && t.sessionStatus().notices.empty(), "steps, kept warm: nothing sounding, no cut: A's silent instances let go as memory needs, nothing said: " +
                  std::to_string(r.releasedSilent - before.releasedSilent) + " let go");
            check(t.warmInstances() == (fitsBefore && room ? 1u : 0u), "steps, kept warm: X's instrument stays kept warm where there is room for it: " + std::to_string(t.warmInstances()));
            t.setBudget({});
        }
        // Back to a Performance after one other, nothing held (FANTOM k6's "again"): A, A again (kept as it is), B, then A. What A
        // left warm at A -> B (or B took over) holds A's state as on main: the return keeps as many as it does with no repeat first.
        // A memory budget everything fits in, as on the device.
        {
            const auto a = tempPath / "steps-back-a", b = tempPath / "steps-back-b", c = tempPath / "steps-back-c", d = tempPath / "steps-back-d";
            ringing(a, 0, argv[1], argv[3], .5); ringing(b, 1, argv[1], argv[3], .5); other(c, argv[2], samples); sampled(d, argv[2], argv[3], samples, false);
            auto back = [&](const fs::path& x, const fs::path& y, bool repeat, bool budget) {
                Session t(48000, frames);
                if(budget) allow(t, uint64_t(1) << 36);
                load(t, x); quiet(t);
                if(repeat) { load(t, x); quiet(t); }
                load(t, y); quiet(t);
                const auto w = load(t, x); std::printf("steps, back: %s%s %s -> %s -> %s: kept %llu beside %llu loaded %llu\n", budget ? "budget, " : "", repeat ? "repeat," : "", x.filename().c_str(), y.filename().c_str(), x.filename().c_str(), (unsigned long long)w.kept, (unsigned long long)w.beside, (unsigned long long)w.loaded);
                return w;
            };
            for(const auto& [x, y] : std::vector<std::pair<fs::path, fs::path>>{{a, b}, {c, a}, {d, a}, {a, c}})
                for(const bool budget : {false, true})
                {
                    const auto plain = back(x, y, false, budget), repeated = back(x, y, true, budget);
                    check(repeated.kept == plain.kept && repeated.beside == plain.beside && repeated.loaded == plain.loaded, "steps, back to A: a repeat kept as it is first changes nothing at the return: " +
                          std::to_string(repeated.kept) + " kept, " + std::to_string(repeated.beside) + " beside against " + std::to_string(plain.kept) + ", " + std::to_string(plain.beside));
                }
        }
        // Short of CPU alone, the silent ones are not let go: carried over and kept warm as before issue 330, so a return to the
        // last Performance takes them back holding its state (FANTOM k6: every silent one let go left nothing warm, kept 0 on the
        // return). Only what sounds may go. Four instruments, a key held on the first; a CPU budget the incoming synth fits with
        // one of them beside it, not with all four; no memory budget.
        {
            ::setenv("OSCAIRN_AUDIO_CORES", "1", 1);
            const auto four = tempPath / "steps-cpu-four"; many(four, 0, argv[4], 4);
            Session t(48000, frames);
            load(t, four); quiet(t);
            t.note(true, 1, .8); blocks(t, 20);
            t.setBudget({64}); t.forgetMeasurements();
            const auto next = t.estimate(incoming.string()); const double one = t.estimate("").plugins.at(0).cpu;
            t.setBudget({next.cpu + 1.5 * one}); t.forgetMeasurements();
            const auto e = t.estimate(incoming.string());
            check(e.fits && !e.seamless && e.fades, "steps, CPU alone: beside all four it does not fit, the memory is there");
            load(t, incoming);
            { Audio x; bool rang = true; for(int i = 0; i < 20; ++i) { x.run(t); rang = rang && x.l[0] == .2f; } check(rang, "steps, CPU alone: the held note rings on, not faded"); }
            const auto r = t.sessionStatus().ringOut;
            check(r.releasedSilent == 0 && r.faded == 0 && r.cut == 0, "steps, CPU alone: nothing silent let go, no fade: " + std::to_string(r.releasedSilent) + " let go");
            t.note(false, 1, 0); quiet(t); quiet(t);
            t.setBudget({});
            const auto w = load(t, four); report("steps: back after CPU alone", w);
            check(w.kept == 4 && w.loaded == 0, "steps, CPU alone: the return takes all four back holding their state: " + std::to_string(w.kept));
            ::setenv("OSCAIRN_AUDIO_CORES", "3", 1);
        }
        // A, an instance that came by claim (its sounds prepared beside, RFC 0021, as the FANTOM's TSO Samplers come from ST
        // Final): it holds the node's sound in another slot, the plugin showing that sound's values once it plays it. The next
        // load of the same Performance, a key held, keeps it as it is, as it keeps one that opened from the file.
        {
            const auto from = tempPath / "steps-claim-a", to = tempPath / "steps-claim-b";
            ringing(from, 0, argv[1], argv[3], .5); ringing(to, 2, argv[1], argv[3], .5);
            Live l; l.serve(4);
            load(l.s, from); l.serve(1200);
            const auto claimed = load(l.s, to); l.serve(1200);
            check(claimed.beside == 1, "steps, a claimed instance: B's synth sound prepared beside A's: " + std::to_string(claimed.beside));
            l.s.controller(64, 127); l.s.note(true, 60, .8); l.serve(20); l.s.note(false, 60, 0); l.serve(20);
            const auto w = load(l.s, to); report("steps: B -> B, B's synth claimed from A, pedaled", w); l.serve(40);
            const auto r = l.s.sessionStatus().ringOut;
            check(w.kept >= 1 && w.beside == 0 && r.kept == 1 && r.chokedOlder == 0, "steps, a claimed instance: kept as it is at the next load of its state, nothing beside it: " +
                  std::to_string(w.kept) + " kept, " + std::to_string(w.beside) + " beside, " + std::to_string(r.kept) + " counted");
            check(l.at(l.block.load() - 2) != 0, "steps, a claimed instance: the pedaled note rings on");
            const auto again = load(l.s, to); l.serve(40);
            check(again.beside == 0 && l.s.sessionStatus().ringOut.chokedOlder == 0 && !l.said("two Performances ago"), "steps, a claimed instance: and again, nothing choked");
            l.s.controller(64, 0);
        }
        // The notice of a tail's chain given up means something audible: an instrument of the last Performance whose sounds the next
        // one prepares beside its own while it is silent, its effect let go for want of room, gives up nothing to hear (FANTOM k5:
        // the notice on every switch with a chord held elsewhere). Two parts apart, each a synth through a Delay; a key held on the
        // first; beside both the incoming pair does not fit, beside the first it does.
        {
            ::unsetenv("OSCAIRN_SHARING");
            auto pair = [&](const fs::path& file, int variant) {
                Session s(48000, frames);
                for(int i = 0; i < 2; ++i)
                {
                    const auto n = s.add("synth " + std::to_string(i), argv[1]), fx = s.add("delay " + std::to_string(i), argv[3]);
                    PartInput keys; keys.zones = {NoteZone{uint8_t(64 * i), uint8_t(64 * i + 63)}};
                    const auto part = s.addPart("Part " + std::to_string(i), keys); s.insertIntoPart(part, 0, n); s.insertIntoPart(part, 1, fx); s.commit();
                    check(s.setParameter(fx, 1, .25) && s.setParameter(fx, 2, .5) && s.setParameter(fx, 3, .6), "delay settings");
                    check(s.selectSound(n, s.prepareSound(n, "Pair " + std::to_string(variant) + std::to_string(i), {{1, .3 + .2 * variant + .1 * i}, {4, double(i)}})), "select a sound");
                }
                blocks(s, 2); s.savePerformance(file.string());
            };
            const auto a = tempPath / "steps-chain-a", b = tempPath / "steps-chain-b"; pair(a, 0); pair(b, 1);
            Live l; l.serve(4);
            load(l.s, a); l.serve(1200);
            l.s.note(true, 30, .8); l.serve(20);
            l.s.setBudget({64}); l.s.forgetMeasurements();
            uint64_t first = 0; for(const auto& c : l.s.estimate("").plugins) if(c.name == "synth 0" || c.name == "delay 0") first += c.memory;
            const auto next = l.s.estimate(b.string()).memory;
            allow(l.s, next + first + first / 4); l.s.forgetMeasurements();
            check(!l.s.estimate(b.string()).seamless, "steps, a silent chain: beside both it does not fit");
            const auto before = l.s.sessionStatus().ringOut;
            load(l.s, b); l.serve(40);
            const auto r = l.s.sessionStatus().ringOut;
            check(r.releasedSilent > before.releasedSilent && r.partial == before.partial && !l.said("without part of their chain") && l.at(l.block.load() - 2) != 0,
                  "steps, a silent chain: the silent part let go says nothing, the held note rings on: " + std::to_string(r.partial - before.partial) + " partial");
            l.s.note(false, 30, 0); l.s.setBudget({});
            ::setenv("OSCAIRN_SHARING", "memory", 1);
        }

        // A: the same Performance loaded again with a key held. Its instrument holds that state already and nothing changed it,
        // so it is kept as it is: nothing is loaded beside it (one copy in memory, where two do not fit), it is not reset, and
        // the held note plays on through the change and ends when its key comes up.
        {
            const auto same = tempPath / "steps-same"; single(same, argv[4], .3);
            Live l; l.serve(4);
            load(l.s, same); l.serve(20);
            l.s.note(true, 60, .8); l.serve(20);
            l.s.setBudget({64}); l.s.forgetMeasurements();
            allow(l.s, l.s.estimate("").memory + l.s.estimate(same.string()).memory - 1); l.s.forgetMeasurements();
            const auto e = l.s.estimate(same.string());
            check(e.fits && e.seamless, "steps, the same Performance again: one copy fits, the instance it keeps counted once: seamless " + std::to_string(e.seamless));
            const auto from = l.block.load();
            const auto w = load(l.s, same); report("steps: the same Performance again, a key held", w);
            l.serve(40);
            check(w.kept == 1 && w.reused == 1 && w.beside == 0 && w.loaded == 0 && l.s.leaving() == 0, "steps, the same Performance again: its instance kept as it is, nothing loaded or prepared beside it, nothing left ringing out: " +
                  std::to_string(w.kept) + " kept, " + std::to_string(w.beside) + " beside, " + std::to_string(w.loaded) + " loaded");
            bool even = true; for(size_t b = from; b < l.block.load() - 1; ++b) for(size_t f = 0; f < frames; ++f) even = even && l.at(b, f) == .3f;
            check(even, "steps, the same Performance again: the held note plays on through the change, every sample at its level");
            check(!l.said("could not ring out") && l.s.sessionStatus().ringOut.cut == 0 && l.s.sessionStatus().ringOut.kept == 1, "steps, the same Performance again: no cut, no notice, counted as kept: " + std::to_string(l.s.sessionStatus().ringOut.kept));
            l.s.note(false, 60, 0); l.serve(10);
            check(l.at(l.block.load() - 2, frames - 1) == 0, "steps, the same Performance again: the key's release reaches the instance kept");
            l.s.setBudget({});
        }
        // Issue 340, FANTOM x1: TSO kept 8 of 19 at every pedaled reload and loaded 11 effects afresh: the chord sounds in one part only,
        // and every effect behind a silent part, or on a bus fed by none, was silent three seconds, which holding leaves to take over, and
        // an effect engine is never taken over. As TSO: a part held through its insert and a send to a bus, a second part silent through an
        // insert of its own. Every effect is kept, sounding or not, and the reload changes not one sample; a reload with nothing held loads
        // nothing either (plain TSO reloads loaded its 13 effects every time). Its -chain variant places it as the FANTOM does.
        {
            ::unsetenv("OSCAIRN_SHARING"); // parts apart, as on the unit
            const auto tso = tempPath / "steps-silent-fx"; chained(tso, argv[1], {argv[3]}, {argv[3]}, .5, .4, true);
            auto run = [&](int reload, Switch* w, size_t* leaving) {
                Session t(48000, frames);
                load(t, tso); quiet(t);
                t.note(true, 30, .8); blocks(t, 40); // the lower part only; the upper one silent three seconds and more
                if(reload) { *w = load(t, tso); *leaving = t.leaving(); }
                std::vector<float> out; Audio a;
                for(int i = 0; i < 300; ++i) { a.run(t); out.insert(out.end(), a.l.begin(), a.l.end()); if(i == 100) t.note(false, 30, 0); }
                return out;
            };
            Switch w; size_t leaving = 0;
            const auto plain = run(0, nullptr, nullptr), reloaded = run(1, &w, &leaving);
            report("steps: a part held, one silent, a bus: reloaded", w);
            size_t differ = 0; for(size_t i = 0; i < plain.size(); ++i) differ += plain[i] != reloaded[i];
            check(w.loaded == 0 && w.reused == 5 && leaving == 0, "steps, silent effects: the silent part's insert kept with the rest, nothing loaded, nothing ringing out beside: " +
                  std::to_string(w.reused) + " reused, " + std::to_string(w.loaded) + " loaded, " + std::to_string(leaving) + " ringing out");
            check(differ == 0, "steps, silent effects: every sample as with no reload: " + std::to_string(differ) + " of " + std::to_string(plain.size()) + " differ");
            Session t(48000, frames);
            load(t, tso); quiet(t);
            const auto again = load(t, tso);
            check(again.loaded == 0, "steps, silent effects: a reload with nothing held loads no effect afresh: " + std::to_string(again.loaded) + " loaded");
            ::setenv("OSCAIRN_SHARING", "memory", 1);
        }
        // Issue 340: the same Performance again with a key held, a synth through a Delay (an engine offering effect contexts). The
        // synth was kept as it is, but the Delay loaded afresh beside the old one ringing out: on the FANTOM eight new effect workers
        // and a second set of tails at every pedaled reload, the callback near its deadline and now and then a late or silent block.
        // Now the Delay is kept too: rendered block by block on this thread, the reload changes not one sample against no reload at
        // all (no click, no tail doubled or restarted). The file saved again (its stamp moved, its wiring might differ) keeps
        // nothing, as before.
        {
            const auto fx = tempPath / "steps-same-fx"; ringing(fx, 0, argv[1], argv[3], .5);
            auto run = [&](int reload, Switch* w, size_t* leaving) {
                Session t(48000, frames);
                load(t, fx); blocks(t, 20);
                t.note(true, 60, .8); blocks(t, 40);
                if(reload == 2) { std::error_code e; fs::last_write_time(fx / "performance.json", fs::last_write_time(fx / "performance.json") + std::chrono::seconds(2), e); }
                if(reload) { *w = load(t, fx); *leaving = t.leaving(); }
                std::vector<float> out; Audio a;
                for(int i = 0; i < 300; ++i) { a.run(t); out.insert(out.end(), a.l.begin(), a.l.end()); if(i == 100) t.note(false, 60, 0); }
                return out;
            };
            Switch w, again; size_t leaving = 0, leavingAgain = 0;
            const auto plain = run(0, nullptr, nullptr), reloaded = run(1, &w, &leaving);
            report("steps: the same Performance again, a key held, through a Delay", w);
            size_t differ = 0; for(size_t i = 0; i < plain.size(); ++i) differ += plain[i] != reloaded[i];
            check(w.kept == 2 && w.loaded == 0 && leaving == 0, "steps, kept effects: the synth and its Delay kept as they are, nothing loaded, nothing ringing out beside: " +
                  std::to_string(w.kept) + " kept, " + std::to_string(w.loaded) + " loaded, " + std::to_string(leaving) + " ringing out");
            check(differ == 0, "steps, kept effects: every sample as with no reload: " + std::to_string(differ) + " of " + std::to_string(plain.size()) + " differ");
            run(2, &again, &leavingAgain);
            check(again.kept == 0 && leavingAgain >= 1, "steps, kept effects: the file saved again keeps nothing unseen (as before): what sounded rings out beside new instances: " +
                  std::to_string(again.kept) + " kept, " + std::to_string(leavingAgain) + " ringing out");
        }
        // Issue 340: engine.timing's open gives the callback's longest block in each phase of the load (the FANTOM's longest callback of a
        // pedaled reload fell in the publish). A thread paced as a device's callback reports each block's time as the test sets it: 100 us
        // while the load runs, 900 once it returned for longer than the 512 blocks the "after" phase lasts, then 2000.
        {
            const auto same = tempPath / "steps-phases", eight = tempPath / "steps-phases-eight"; single(same, argv[4], .3); many(eight, 1, argv[4], 8); // eight to load: a read of some blocks
            std::atomic<int64_t> took{100'000};
            Live l; std::atomic<bool> stop{false};
            std::thread clock([&] { int64_t at = 0; size_t seen = 0; while(!stop) { for(const auto b = l.block.load(); seen < b; ++seen) { l.s.blockTimes(at, at + took.load(), 2'666'667); at += 2'666'667; } std::this_thread::sleep_for(std::chrono::microseconds(200)); } });
            l.serve(4); load(l.s, same); l.serve(600);
            load(l.s, eight); took = 900'000; l.serve(600);
            const auto o = l.s.engineTiming(false).open;
            took = 2'000'000; l.serve(100);
            const auto later = l.s.engineTiming(false).open;
            stop = true; clock.join();
            std::printf("steps, callback by phase: read %.0f, publish %.0f, after %.0f us; 100 blocks on, after %.0f us\n", o.callbackRead, o.callbackPublish, o.callbackAfter, later.callbackAfter);
            check(o.callbackRead == 100 && o.callbackPublish <= 100 && o.callbackAfter == 900, "steps, callback by phase: each phase its own longest block: read " + std::to_string(o.callbackRead) + ", after " + std::to_string(o.callbackAfter));
            check(later.callbackAfter == 900 && later.callbackRead == 100, "steps, callback by phase: after ends with its 512 blocks: " + std::to_string(later.callbackAfter));
        }
        // B: four instruments, one key held on the first. Beside all four it does not fit, beside the one sounding it does: the
        // three silent ones go, the held note rings on, and nothing is said.
        {
            const auto four = tempPath / "steps-four"; many(four, 0, argv[4], 4);
            Live l; l.serve(4);
            load(l.s, four); l.serve(1200); // three seconds of silence: free
            l.s.note(true, 1, .8); l.serve(20);
            l.s.setBudget({64}); l.s.forgetMeasurements();
            const auto one = l.memoryOf(0), next = l.s.estimate(incoming.string()).memory;
            allow(l.s, next + one + one / 2); l.s.forgetMeasurements();
            check(!l.s.estimate(incoming.string()).seamless, "steps, silent ones: beside all four it does not fit");
            const auto from = l.block.load();
            load(l.s, incoming); l.serve(40);
            check(l.has("synth"), "steps, silent ones: the new Performance loaded");
            bool rang = true; for(size_t b = from; b < l.block.load() - 1; ++b) rang = rang && l.at(b) == .2f;
            check(rang, "steps, silent ones: the held note rings on through the change: " + std::to_string(l.at(l.block.load() - 2)));
            const auto r = l.s.sessionStatus().ringOut;
            check(!l.said("could not ring out") && r.cut == 0 && r.releasedSilent == 3 && r.releasedFaded == 0 && l.s.leaving() == 1, "steps, silent ones: the three silent ones let go, said nowhere, the sounding one ringing out: " +
                  std::to_string(r.releasedSilent) + " let go, " + std::to_string(l.s.leaving()) + " ringing");
            l.s.note(false, 1, 0); l.s.setBudget({});
        }
        // B and C: two instruments both held, the first with three keys, the second with one. Beside both it does not fit, beside
        // the first it does: the second (fewer notes) fades out in 150 ms and goes, the first rings on, and a notice says so.
        {
            const auto two = tempPath / "steps-two"; many(two, 0, argv[4], 2);
            Live l; l.serve(4);
            load(l.s, two); l.serve(1200); // three seconds of silence: free
            for(const int key : {0, 1, 2}) l.s.note(true, key, .8);
            l.s.note(true, 4, .8); l.serve(20);
            l.s.setBudget({64}); l.s.forgetMeasurements();
            const auto one = l.memoryOf(0), next = l.s.estimate(incoming.string()).memory;
            allow(l.s, next + one + one / 2); l.s.forgetMeasurements();
            const auto from = l.block.load();
            const float both = l.at(from - 1);
            load(l.s, incoming); l.serve(60);
            check(l.has("synth"), "steps, partly: the new Performance loaded");
            const auto t = transition(l, from, l.block.load() - 1, both, .2f);
            std::printf("steps, partly: %.3f to %.3f, %zu samples between, the largest step %.5f\n", both, .2, t.between, t.step);
            check(std::abs(both - .41f) < 1e-5f, "steps, partly: both held before the change: " + std::to_string(both));
            check(t.reached, "steps, partly: the first, with more notes held, rings on at its level");
            check(t.between >= 20 * frames && t.step <= .05f * .21f, "steps, partly: the second faded out, not cut: " + std::to_string(t.between) + " samples between, a step of " + std::to_string(t.step));
            const auto r = l.s.sessionStatus().ringOut;
            check(l.said("faded out in 150 ms") && l.said("instrument 1") && r.releasedFaded == 1 && r.cut == 0, "steps, partly: the notice names the one faded out, counted: " + std::to_string(r.releasedFaded));
            for(const int key : {0, 1, 2, 4}) l.s.note(false, key, 0);
            l.s.setBudget({});
        }
        // C: one instrument held, and it does not fit beside even that: the whole of it goes, faded out in 150 ms (a cut of
        // about 1 ms before), said; the new Performance loads.
        {
            const auto lone = tempPath / "steps-lone"; many(lone, 0, argv[4], 1);
            Live l; l.serve(4);
            load(l.s, lone); l.serve(1200); // three seconds of silence: free
            l.s.note(true, 1, .8); l.serve(20);
            l.s.setBudget({64}); l.s.forgetMeasurements();
            const auto one = l.memoryOf(0), next = l.s.estimate(incoming.string()).memory;
            allow(l.s, next + one / 2); l.s.forgetMeasurements();
            const auto from = l.block.load();
            load(l.s, incoming); l.serve(60);
            check(l.has("synth"), "steps, a cut: the new Performance loaded");
            const auto t = transition(l, from, l.block.load() - 1, .2f, 0);
            std::printf("steps, a cut: %zu samples between, the largest step %.5f\n", t.between, t.step);
            check(t.reached && t.between >= 20 * frames && t.step <= .05f * .2f, "steps, a cut: faded out, not cut: " + std::to_string(t.between) + " samples between, a step of " + std::to_string(t.step));
            check(l.said("could not ring out") && l.said("faded out in 150 ms") && l.s.sessionStatus().ringOut.cut == 1, "steps, a cut: said, and counted as a cut");
            l.s.note(false, 1, 0); l.s.setBudget({});
        }
    }
    }
}

int main(int argc, char** argv)
{
    if(argc != 5) { std::fprintf(stderr, "usage: %s SYNTH.clap SAMPLER.clap DELAY.clap INSTRUMENT.clap\n", argv[0]); return 2; }
    try
    {
        state_files::TemporaryDirectory home; // a budget's measurements, not the user's
        ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
        ::setenv("OSCAIRN_SHARING", "memory", 1); // RFC 0045: what changes do to instances parts share, as before parts went apart by default
        // Every Session here with the load guard off: switched and fresh are compared sample for sample, which tests
        // rendering, not load; on a machine too slow for real time (a sanitizer build) the guard gives notes up unevenly.
        ::setenv("OSCAIRN_LOAD_GUARD", "off", 1);
        std::printf("plugins in %s\n", placementPolicy().worker.empty() ? "the engine" : "workers");
        state_files::TemporaryDirectory temp;
        const auto samples = temp.path / "samples"; fs::create_directories(samples);
        { std::vector<float> wave(48000); for(size_t f = 0; f < wave.size(); ++f) wave[f] = float(.3 * ((f * 3) % 200) / 200.0 - .15); writeWavFile(samples / "sample.wav", wave, 1, 48000); }
        const auto a = temp.path / "a", b = temp.path / "b", c = temp.path / "c";
        shared(a, 0, argv[1], argv[3], argv[4]); shared(b, 1, argv[1], argv[3], argv[4]); other(c, argv[2], samples);

        if(std::getenv("OSCAIRN_SWITCH_STEPS"))
        {
            ::setenv("OSCAIRN_AUDIO_CORES", "3", 1); // the FANTOM-07's, as the fit rule's cases above run
            { std::vector<float> wave(4 * 48000); for(size_t f = 0; f < wave.size(); ++f) wave[f] = float(.3 * ((f * 3) % 200) / 200.0 - .15); writeWavFile(samples / "long.wav", wave, 1, 48000); }
            issue330(argv, temp.path, samples);
            if(failures) return 1;
            std::printf("ok: issue 330's steps\n");
            return 0;
        }
        // RFC 0045: residents cost nothing but memory, and give it back. Y's instrument (a copy of the
        // plugin's build, replaced later) and C's four Samplers kept warm while G plays, paced as a device's
        // callback; their workers use next to no CPU. Then the system's available memory (a /proc/meminfo
        // of the test's) falls under the budget's floor: the oldest and largest go until the shortfall is
        // let go, those in use stay. Y's build replaced on disk: no load can take its instance, so it goes
        // within a second. memory.release lets go of the rest.
        // Its own test (control-residents-workers, OSCAIRN_SWITCH_RESIDENTS): the rest of this one is long enough.
        if(std::getenv("OSCAIRN_SWITCH_RESIDENTS"))
        {
            if(placementPolicy().worker.empty() || placementPolicy().hosts) throw std::runtime_error("residents are checked with a worker per instance");
            const auto copy = temp.path / ("copy-" + fs::path(argv[4]).filename().string());
            fs::copy(argv[4], copy, fs::copy_options::recursive);
            const auto y = temp.path / "resident-y", g = temp.path / "resident-g";
            single(y, copy.string().c_str(), .5); ringing(g, 0, argv[1], argv[3], .5);
            std::set<long> earlier; for(const auto& h : pluginHosts()) earlier.insert(h.pid); // other Sessions' workers
            Session t(48000, frames);
            auto gone = [&] { for(int i = 0; i < int(48000 * 31 / frames) && t.leaving(); ++i) blocks(t, 1); };
            auto paced = [&](double seconds, const std::function<bool()>& until = {}) {
                const auto end = std::chrono::steady_clock::now() + std::chrono::duration<double>(seconds);
                while(std::chrono::steady_clock::now() < end && !(until && until())) { blocks(t, 1); std::this_thread::sleep_for(std::chrono::microseconds(2667)); }
            };
            load(t, y); quiet(t); load(t, c); gone(); quiet(t); load(t, g); gone();
            check(t.warmInstances() == 5, "Y's instrument and C's Samplers kept warm: " + std::to_string(t.warmInstances()));
            std::vector<long> resident;
            for(const auto& h : pluginHosts())
                if(!earlier.count(h.pid) && !h.plugins.empty() && std::none_of(h.plugins.begin(), h.plugins.end(), [](const std::string& p) { return p == "org.oscairn.basic-synth" || p == "org.oscairn.delay"; })) resident.push_back(h.pid);
            check(resident.size() == 5, "five workers kept warm: " + std::to_string(resident.size()));
            paced(.5); // past their last request
            // Other load on the machine (a shared runner) only adds CPU time to a worker, never takes any away: the least of up to three windows is its own.
            const double limit = relaxed() ? .030 : .018; // 0.5% of a core on a CI runner or in a sanitizer build, 0.3% elsewhere
            double most = 1e9; bool measurable = true;
            for(int window = 0; window < 3 && most > limit; ++window)
            {
                std::vector<double> before; for(const auto pid : resident) before.push_back(cpuOf(pid));
                paced(6);
                double busiest = 0; for(size_t i = 0; i < resident.size(); ++i) busiest = std::max(busiest, cpuOf(resident[i]) - before[i]);
                most = std::min(most, busiest);
                if(before.empty() || before[0] < 0) { measurable = false; break; }
            }
            std::printf("residents: the busiest of %zu workers kept warm used %.1f ms of CPU in 6 s while G played\n", resident.size(), most * 1e3);
            check(!measurable || most <= limit, "a worker kept warm costs next to no CPU: at most 0.3% of a core (0.5% on a CI runner or in a sanitizer build: GitHub's macOS runners measure 0.3 to 0.4%), " + std::to_string(most * 1e3) + " ms in 6 s");
            // Y's build replaced (a library install): no load can take its instance kept warm, which goes.
            auto idle = [&] { std::map<std::string, size_t> n; for(const auto& i : t.engineTiming(false).instances) if(!i.inUse) ++n[i.plugin]; return n; };
            std::string yPlugin; for(const auto& [id, n] : idle()) if(n == 1) yPlugin = id;
            fs::path binary = copy; // a bundle's binary (macOS), else the file
            if(fs::is_directory(copy)) for(const auto& f : fs::recursive_directory_iterator(copy)) if(f.is_regular_file() && f.path().parent_path().filename() == "MacOS") binary = f.path();
            { std::ofstream f(binary, std::ios::app | std::ios::binary); f << "a new build"; }
            paced(3, [&] { return t.warmInstances() < 5; });
            check(!yPlugin.empty() && t.warmInstances() == 4 && idle().count(yPlugin) == 0, "the instance of a build replaced on disk let go, the others kept: " + std::to_string(t.warmInstances()));
            // The floor: 1 GiB to keep available, 1 KiB short of it.
            const auto meminfo = temp.path / "meminfo";
            auto available = [&](uint64_t kb) { std::ofstream(meminfo) << "MemTotal:       8000000 kB\nMemFree:         100000 kB\nMemAvailable:   " << kb << " kB\n"; };
            available((uint64_t(1) << 20) - 1);
            ::setenv("OSCAIRN_MEMINFO", meminfo.c_str(), 1);
            Session::Budget floor{64}; floor.available = uint64_t(1) << 30;
            t.setBudget(floor);
            auto listed = [&](bool inUse) { std::map<std::string, size_t> n; for(const auto& i : t.engineTiming(false).instances) if(i.inUse == inUse) ++n[i.plugin]; return n; };
            const auto used = listed(true);
            paced(2.5, [&] { return t.warmInstances() < 4; });
            check(t.warmInstances() == 3 && listed(true) == used, "1 KiB short of the floor: one kept warm let go, none in use: " + std::to_string(t.warmInstances()));
            check(t.memoryState().available == floor.available && t.memoryState().systemAvailable == (((uint64_t(1) << 20) - 1) << 10), "memory.state gives the floor and what is available");
            available(uint64_t(4) << 20); // above it: nothing more goes
            paced(3.5);
            check(t.warmInstances() == 3, "above the floor, the rest stay: " + std::to_string(t.warmInstances()));
            t.setBudget({});
            ::unsetenv("OSCAIRN_MEMINFO");
            const auto left = t.warmInstances();
            const auto r = t.releaseResidents();
            check(r.instances == left && r.bytes > 0 && t.warmInstances() == 0 && listed(true) == used, "memory.release lets go of every instance kept warm, none in use: " + std::to_string(r.instances));
            // A Performance about to load with too little available: those kept warm it does not use go
            // first, those it may take stay. C (four Samplers, never measured: its estimate their default
            // guess) opened with exactly the floor available: Y's instrument goes, the Samplers are taken over.
            Session u(48000, frames);
            auto rung = [&] { for(int i = 0; i < int(48000 * 31 / frames) && u.leaving(); ++i) blocks(u, 1); };
            load(u, y); quiet(u); load(u, c); rung(); quiet(u); load(u, g); rung();
            check(u.warmInstances() == 5, "Y's instrument and C's Samplers kept warm: " + std::to_string(u.warmInstances()));
            // Relaxed: 1 KiB short of the floor, so the Samplers' own memory (more than the estimate, in a sanitizer build) is never enough to need none freed.
            available(relaxed() ? (uint64_t(1) << 20) - 1 : uint64_t(1) << 20);
            ::setenv("OSCAIRN_MEMINFO", meminfo.c_str(), 1);
            u.setBudget(floor);
            const auto w = load(u, c); report("G -> C, the floor available", w);
            check(w.loaded == 0 && w.reused == 4 && u.warmInstances() == 0, "the instrument let go before the read, the Samplers taken over: " + std::to_string(w.loaded) + " loaded, " +
                  std::to_string(u.warmInstances()) + " kept warm");
            u.setBudget({});
            ::unsetenv("OSCAIRN_MEMINFO");
            if(failures) return 1;
            std::printf("ok: residents cost no CPU and give their memory back\n");
            return 0;
        }


        Session s(48000, frames);
        report("open A (cold)", load(s, a));
        blocks(s, 20); s.note(true, 60, 1); blocks(s, 20); s.note(false, 60, 0); quiet(s);
        // A is silent and B uses the same plugins. A's two Basic Synths share one instance (formed on
        // their own, slice 3), which gets B's two synths' sounds beside A's. A's plain instruments
        // would be kept warm (RFC 0021's middle rule), but an instance is taken over before a new
        // process is started (RFC 0045's resident instances): B loads nothing, and its Delays become
        // contexts of A's (slice 1).
        auto w = load(s, b); report("A -> B, A silent (same plugins)", w);
        check(w.reused == 8 && w.beside == 0 && w.loaded == 0, "every instance of A taken over by B, none loaded");
        compare(s, b, "A -> B"); quiet(s);
        check(s.warmInstances() == 0, "nothing of A left to keep warm: " + std::to_string(s.warmInstances()));
        // Back to A, and to B again: no process started either way; the instances get the state again.
        w = load(s, a); report("B -> A, B silent", w);
        check(w.reused == 8 && w.beside == 0 && w.kept == 0 && w.loaded == 0, "back to A: B's instances taken over, none loaded");
        compare(s, a, "B -> A"); quiet(s);
        w = load(s, b); report("A -> B again", w);
        check(w.reused == 8 && w.beside == 0 && w.kept == 0 && w.loaded == 0, "A -> B again: none loaded");
        compare(s, b, "A -> B again"); quiet(s);
        // A cut (the two would not fit together) keeps nothing warm, so it takes over every free
        // playing instance.
        s.setBudget({64, 0});
        const Session::Budget cut{64, s.estimate("").memory + s.estimate(a.string()).memory - 1};
        s.setBudget(cut);
        check(!s.estimate(a.string()).seamless, "B -> A cuts");
        w = load(s, a); report("B -> A, a cut", w);
        check(w.reused == 8 && w.loaded == 0 && w.kept == 0, "a cut takes over every free playing instance");
        auto cutSaid = [&] { for(const auto& n : s.sessionStatus().notices) if(n.find("notes could not ring out") != std::string::npos) return true; return false; };
        check(!cutSaid(), "a cut with nothing sounding says nothing of notes");
        // Off while this thread renders: a budget measures each block's wall time as its plugins' cost,
        // and this is no real-time thread, so on a loaded machine a block descheduled would count.
        s.setBudget({});
        compare(s, a, "cut B -> A"); quiet(s);
        // A cut takes over the instance a held note sounds through too: the reused instance carries
        // nothing over (voices, held notes, state), so B then sounds as B opened fresh. Then back to A.
        // B fits alone and beside nothing of A (issue 330: what does fit beside it rings on, below).
        s.note(true, 40, 1); blocks(s, 10);
        s.setBudget({64, 0}); allow(s, s.estimate(b.string()).memory);
        check(!s.estimate(b.string()).seamless, "A -> B cuts");
        w = load(s, b); report("A -> B, a cut, a note held", w);
        check(w.reused == 8 && w.loaded == 0 && s.leaving() == 0, "a cut takes over the sounding instance too, nothing rings out");
        check(cutSaid(), "a cut of a held note is said: the last Performance's notes could not ring out");
        s.setBudget({});
        s.note(false, 40, 0);
        compare(s, b, "a cut with a note held"); quiet(s);
        w = load(s, a); quiet(s);
        check(w.loaded == 0, "back to A, nothing loaded");
        // A document that fails once its plugins are loaded leaves the playing instances alone:
        // they get its state only once all of it has been read.
        {
            const auto broken = temp.path / "broken"; fs::copy(b, broken, fs::copy_options::recursive);
            auto bytes = state_files::read(broken / "performance.json", 4 << 20); std::string text(bytes.begin(), bytes.end());
            const auto at = text.find("\"scene\":0"); check(at != std::string::npos, "the broken copy's selected scene");
            text.replace(at, 9, "\"scene\":9"); state_files::write(broken / "performance.json", text.data(), text.size());
            auto values = [&] { std::vector<double> v; for(const auto& [n, name] : s.graphState().names) for(const auto& p : s.parameters(n)) v.push_back(s.parameter(n, p.id)); return v; };
            auto sounds = [&] { size_t n = 0; for(const auto& [id, name] : s.graphState().names) n += s.sounds(id).contexts.size(); return n; };
            const auto before = values(); const auto revision = s.graphState().revision; const auto held = sounds(); bool failed = false;
            s.setBudget(cut);
            try { s.loadPerformance(broken.string()); } catch(const std::exception&) { failed = true; }
            check(failed && s.graphState().revision == revision && values() == before, "a failed load changes nothing that plays");
            check(sounds() == held, "a failed load leaves no sound prepared beside: " + std::to_string(held) + " then " + std::to_string(sounds()));
        }
        // A load that fails in that last step, its fourth instrument refusing its state even in a
        // fresh instance, once the first three playing instances it took over were given theirs:
        // they get their own back, so A sounds exactly as before the attempt.
        {
            const auto refusing = temp.path / "refusing"; fs::copy(b, refusing, fs::copy_options::recursive);
            const auto bytes = state_files::read(refusing / "performance.json", 4 << 20); const std::string text(bytes.begin(), bytes.end());
            const auto name = text.find("\"name\":\"instrument 1-3\""), key = text.rfind("\"key\":\"", name);
            check(name != std::string::npos && key != std::string::npos, "the fourth instrument's key");
            const auto node = text.substr(key + 7, text.find('"', key + 7) - key - 7);
            const std::string bad = "state 1\nnot a snapshot";
            state_files::write(refusing / "nodes" / node / "snapshot", bad.data(), bad.size());
            s.setBudget({64, s.estimate("").memory + s.estimate(refusing.string()).memory - 1}); // a cut: it takes every free instance
            check(!s.estimate(refusing.string()).seamless, "A -> the refusing copy cuts");
            bool failed = false;
            try { s.loadPerformance(refusing.string()); } catch(const std::exception& e) { failed = std::string(e.what()).find("instrument 1-3") != std::string::npos; }
            check(failed, "the load fails on the fourth instrument");
            // Off while this thread renders, as before the other comparisons: a budget keeps the load guard on whatever OSCAIRN_LOAD_GUARD says,
            // and a block descheduled on a loaded machine gives a note up in the one Session and not in the fresh one.
            s.setBudget({});
            compare(s, a, "a load refused after taking instances over"); quiet(s);
        }
        s.setBudget({});
        // A note held on the second part through the change (seamless: no budget): its instrument and
        // Delay ring out on the old instances, so its instrument loads afresh (its Delay becomes a
        // context of the playing engine, slice 1); the other parts' are taken over.
        s.note(true, 60, 1); blocks(s, 10);
        w = load(s, b); report("A -> B, a note held on A's second part", w);
        check(w.reused == 7 && w.beside == 0 && w.loaded == 1, "A's silent instances taken over, the sounding one not: one loaded beside it");
        { Audio audio; bool rang = false; for(int i = 0; i < 50; ++i) { audio.run(s); rang = rang || audio.sounding(); } check(rang, "the held note plays on through the change"); }
        s.note(false, 60, 0);
        for(int i = 0; i < int(48000 * 31 / frames) && s.leaving(); ++i) blocks(s, 1);
        check(!s.leaving(), "the old instances go once rung out");
        // Nothing shared.
        w = load(s, c); report("B -> C (no plugin shared)", w);
        check(w.reused == 0 && w.loaded == 4, "nothing to take over");
        for(int i = 0; i < int(48000 * 31 / frames) && s.leaving(); ++i) blocks(s, 1);
        // B's plain instruments, rung out, were kept warm, with the one A's held note kept sounding
        // (Delays, engines with contexts, are not; nor is a shared synth instance, which goes with its
        // last node).
        check(s.warmInstances() == 3, "B's instruments and A's sounding one kept warm once rung out: " + std::to_string(s.warmInstances()));
        {
            // engine.timing lists them idle, the rest in use.
            const auto t = s.engineTiming(false);
            const auto idle = std::count_if(t.instances.begin(), t.instances.end(), [](const auto& i) { return !i.inUse && i.idle > 0; });
            check(idle == 3 && t.instances.size() == 7, "engine.timing lists the instances, those kept warm idle: " + std::to_string(idle) + " of " + std::to_string(t.instances.size()));
        }
        // Back to B: its plain ones hold its state already, so they are reset instead of given it
        // again; B's shared synth loads one instance.
        w = load(s, b); report("C -> B (B's instruments kept warm)", w);
        check(w.reused == 2 && w.kept == 2 && w.loaded == 5 && s.warmInstances() == 1, "the instances kept warm taken over, not given their state again; the shared synth and the Delays loaded");
        compare(s, b, "kept warm -> B");
        for(int i = 0; i < int(48000 * 31 / frames) && s.leaving(); ++i) blocks(s, 1);
        // Memory short: what is kept warm goes first (RFC 0025).
        check(s.warmInstances() == 5 && s.memoryPressure() && s.warmInstances() == 0, "C's Samplers and A's instrument kept warm, let go on memory pressure");

        // 34 parts, one holding a note: the 33 free instances are taken over, the sounding one never;
        // it rings out and is kept warm. Then to C, which shares nothing: 32 of the 35 are kept warm,
        // the bound. How long the change takes, every instance loaded and every one taken over.
        {
            const auto m = temp.path / "m", n = temp.path / "n";
            many(m, 0, argv[4], 34); many(n, 1, argv[4], 34);
            Session t(48000, frames);
            const auto spawned = load(t, m); report("34 instruments, all loaded", spawned); quiet(t);
            t.note(true, 1, 1); blocks(t, 10);
            w = load(t, n); report("34 instruments, one sounding", w);
            check(w.reused == 33 && w.loaded == 1, "the free instances taken over, the sounding one not");
            t.note(false, 1, 0);
            for(int i = 0; i < int(48000 * 31 / frames) && t.leaving(); ++i) blocks(t, 1);
            check(t.warmInstances() == 1, "the sounding one kept warm once rung out: " + std::to_string(t.warmInstances()));
            const auto reused = load(t, m); report("34 instruments, all taken over", reused);
            check(reused.reused == 34 && reused.loaded == 0, "back to M: every instance taken over, none loaded");
            std::printf("34 instruments: %.1f ms loading every instance, %.1f ms taking every one over\n", spawned.ms, reused.ms);
            load(t, c);
            for(int i = 0; i < int(48000 * 31 / frames) && t.leaving(); ++i) blocks(t, 1);
            check(t.warmInstances() == 32, "32 kept warm: " + std::to_string(t.warmInstances()));
        }

        // RFC 0045: with memory classes, instances kept warm are bounded by memory, not by count, and let
        // go least recently used first, their workers ending on a thread of their own; one in use never.
        // X's instrument is kept warm first, then C's four Samplers; G (a synth through a Delay) plays.
        // Only workers report their memory, so only there.
        if(!placementPolicy().worker.empty())
        {
            const auto x = temp.path / "evict-x", g = temp.path / "evict-g";
            single(x, argv[4], .5); ringing(g, 0, argv[1], argv[3], .5);
            Session t(48000, frames);
            auto gone = [&] { for(int i = 0; i < int(48000 * 31 / frames) && t.leaving(); ++i) blocks(t, 1); };
            load(t, x); quiet(t); load(t, c); gone(); quiet(t); load(t, g); gone();
            check(t.warmInstances() == 5, "X's instrument and C's Samplers kept warm: " + std::to_string(t.warmInstances()));
            auto listed = [&](bool inUse) { std::map<std::string, size_t> n; for(const auto& i : t.engineTiming(false).instances) if(i.inUse == inUse) ++n[i.plugin]; return n; };
            const auto idle = listed(false), used = listed(true);
            check(idle.size() == 2 && !used.empty(), "two plugins idle, G's in use");
            std::string instrument; for(const auto& [id, n] : idle) if(n == 1) instrument = id;
            Session::Budget b{64, 0, uint64_t(1) << 40, 0, 0, 0};
            t.setBudget(b); blocks(t, 2);
            const auto m = t.memoryState();
            check(m.opportunistic > 0 && m.committed > 0, "the warm instances' memory and G's measured: " + std::to_string(m.opportunistic) + ", " + std::to_string(m.committed));
            const auto workers = children();
            // Short by half the least recently used one's memory: it goes, and only it. What the session holds reserved counts
            // (the worker program's code, every worker's: workerCode), which a budget of committed and opportunistic alone,
            // one byte short, left out: a code bigger than the instrument's memory (a sanitizer's worker) let two go. Half the
            // instrument's memory either way, so readings that move between this one and the session's own decide nothing.
            uint64_t oldest = 0;
            for(const auto& i : t.engineTiming(false).instances) if(!i.inUse && i.plugin == instrument) oldest = i.memory;
            b.device = m.reserved + m.committed + m.opportunistic - std::max<uint64_t>(1, oldest / 2);
            const auto at = std::chrono::steady_clock::now();
            t.setBudget(b);
            const auto ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - at).count();
            check(t.warmInstances() == 4 && listed(false).count(instrument) == 0 && listed(true) == used, "X's instrument, the least recently used, let go; G's in use kept: " +
                  std::to_string(t.warmInstances()) + " kept warm; committed " + std::to_string(m.committed) + ", opportunistic " + std::to_string(m.opportunistic) + ", X's " + std::to_string(oldest));
            bool ended = false;
            for(int i = 0; i < 50 && !ended; ++i) { ended = children() < workers; if(!ended) std::this_thread::sleep_for(std::chrono::milliseconds(20)); }
            if(!placementPolicy().hosts) check(ended, "its worker ended in the background"); // a plugin host stays (RFC 0045)
            std::printf("evicting one instance kept warm held the control thread %.2f ms\n", ms);
            b.device = m.committed; // no room at all: every instance kept warm goes, none in use
            t.setBudget(b);
            check(t.warmInstances() == 0 && listed(true) == used, "no room: every instance kept warm let go, none in use");
            t.setBudget({});
            // Before a Performance is read, room for it: an instance kept warm that it cannot take goes
            // first, though newer, and those it may take stay. C's Samplers kept warm, then X's instrument;
            // C opened with room for all but the instrument takes the four Samplers and loads nothing.
            Session u(48000, frames);
            auto rung = [&] { for(int i = 0; i < int(48000 * 31 / frames) && u.leaving(); ++i) blocks(u, 1); };
            load(u, c); quiet(u); load(u, x); rung(); quiet(u); load(u, g); rung();
            check(u.warmInstances() == 5, "C's Samplers, then X's instrument, kept warm: " + std::to_string(u.warmInstances()));
            uint64_t instrumentBytes = 0, samplerBytes = 0;
            for(const auto& i : u.engineTiming(false).instances) if(!i.inUse) (i.plugin == instrument ? instrumentBytes : samplerBytes) += i.memory;
            Session::Budget room{64, 0, uint64_t(1) << 40, 0, 0, 0};
            u.setBudget(room); blocks(u, 2);
            const auto n = u.memoryState();
            // C's new instances: its estimate less the Samplers kept warm it takes over (issue 9: they are not counted twice).
            const auto estimate = u.estimate(c.string()).memory, fresh = estimate - std::min(estimate, samplerBytes);
            std::printf("G -> C: C's estimate %llu bytes, the Samplers kept warm %llu, its new %llu, the instrument %llu\n", (unsigned long long)estimate, (unsigned long long)samplerBytes, (unsigned long long)fresh, (unsigned long long)instrumentBytes);
            // A sanitizer's workers hold several times the estimate's guess: there C needs nothing new and the case cannot be made.
            if(fresh <= instrumentBytes) std::printf("G -> C: not checked, C needs no more new memory than the instrument holds\n");
            else
            {
                // Half the instrument's memory short of room for everything, what is reserved counted (as above): C fits beside its
                // Samplers by that much, and with the instrument would not by as much. Without the reserve and exactly at the edge,
                // a Sampler went too, or the instrument before anything was opened (ThreadSanitizer, 2 of 5).
                room.device = n.reserved + n.committed + fresh + n.opportunistic - instrumentBytes / 2;
                u.setBudget(room);
                check(u.warmInstances() == 5, "all kept warm while nothing is opened");
                w = load(u, c); report("G -> C, room for C but not the instrument kept warm", w);
                check(w.loaded == 0 && w.reused == 4 && u.warmInstances() == 0, "the instrument let go before the read, the Samplers taken over: " + std::to_string(w.loaded) + " loaded");
            }
            u.setBudget({});
        }

        // A taken-over instance plays as a fresh load of its state would: what the outgoing Performance
        // left in it is cleared (CLAP reset), as a state load alone need not. P leaves a voice in its
        // instrument below the silence a takeover waits for (the fixture's KICK_FIXTURE_STUCK: a voice
        // no release or state load ends): it must not sound in Q, which takes P's instance kept warm.
        {
            const auto p = temp.path / "stuck-p", q = temp.path / "stuck-q";
            single(p, argv[4], 1e-6); single(q, argv[4], .5);
            ::setenv("KICK_FIXTURE_STUCK", "1", 1);
            Session t(48000, frames);
            load(t, p); t.note(true, 60, 1); blocks(t, 10); t.note(false, 60, 0); quiet(t);
            load(t, c); // shares nothing: P's instance rings out and is kept warm
            for(int i = 0; i < int(48000 * 31 / frames) && t.leaving(); ++i) blocks(t, 1);
            w = load(t, q); report("stuck: C -> Q (P's instance kept warm)", w);
            check(w.reused == 1 && w.kept == 0 && w.loaded == 0, "Q takes over P's instance kept warm");
            compare(t, q, "stuck: P's instance kept warm, then Q's");
            ::unsetenv("KICK_FIXTURE_STUCK");
        }

        // A Performance's levels start at its own, as when it opens in a new Session, not ramp from
        // the last one's in the block the change is heard in: L's part and master are low, M's full.
        {
            const auto l = temp.path / "levels-l", m = temp.path / "levels-m";
            single(l, argv[4], .5, .25, .5); single(m, argv[4], .5);
            Session t(48000, frames);
            load(t, l); blocks(t, 10);
            load(t, m); compare(t, m, "levels: L -> M", 0); quiet(t);
            load(t, l); compare(t, l, "levels: M -> L", 0);
        }

        // A shared engine claimed across a change (RFC 0021, slice 3): D and E are two Basic Synth
        // parts each, on one instance each (formed on their own), with other sounds. E's sounds are
        // prepared beside D's in D's instance, each on a channel and bus of its own, and selected at
        // the cue: nothing is loaded, no state is given, a note held on D rings out in its own sound
        // through its own node, then D's nodes go and their sounds are discarded, so changing back and
        // forth keeps fitting the instance's eight sounds.
        {
            const auto d = temp.path / "d", e = temp.path / "e", f = temp.path / "f";
            synths(d, 0, argv[1]); synths(e, 1, argv[1]);
            crowd(f, argv[1], 12); // twelve parts: more than an instance holds
            Session t(48000, frames);
            auto engines = [&] { std::set<NodeId> n; for(const auto& [id, x] : t.graphState().shared) n.insert(x.engine); return n.size(); };
            const auto opened = pluginInstancesLoaded();
            report("synths: open D (cold)", load(t, d)); quiet(t);
            check(pluginInstancesLoaded() - opened == 1 && engines() == 1 && t.graphState().shared.size() == 2, "D's two synth parts on one instance");
            t.note(true, 20, 1); blocks(t, 10); // held on D's first part through the change
            w = load(t, e); report("synths: D -> E, a note held on D's first part", w);
            check(w.loaded == 0 && w.reused == 2 && w.beside == 0 && w.kept == 0, "E's sounds prepared beside D's in D's instance: nothing loaded, no state given");
            check(t.leaving() == 2 && engines() == 1, "D's nodes ring out on the instance E's play on");
            blocks(t, 20);
            {
                Audio audio; bool rang = false; for(int i = 0; i < 20; ++i) { audio.run(t); rang = rang || audio.sounding(); }
                check(rang, "the held note plays on through the change");
                check(t.leaving() >= 1, "the held note's node still rings out");
            }
            t.note(false, 20, 0);
            quiet(t);
            auto gone = [&] { for(int i = 0; i < int(48000 * 31 / frames) && t.leaving(); ++i) blocks(t, 1); };
            gone();
            check(!t.leaving(), "D's nodes gone once rung out");
            compare(t, e, "synths D -> E"); gone();
            // Changing back and forth fits only if each outgoing Performance's sounds were discarded.
            for(int round = 0; round < 2; ++round)
            {
                w = load(t, d); report("synths: E -> D, E silent", w);
                check(w.loaded == 0 && w.reused == 2, "back to D: its sounds beside E's, nothing loaded");
                compare(t, d, "synths E -> D"); gone();
                w = load(t, e); report("synths: D -> E, D silent", w);
                check(w.loaded == 0 && w.reused == 2, "D -> E again: E's sounds beside D's, nothing loaded");
                compare(t, e, "synths D -> E again"); gone();
            }
            // Prepared ahead: E's sounds go beside while D plays, and the cue selects them.
            w = load(t, d); gone();
            check(t.preparePerformance(e.string()), "E prepared ahead");
            w = load(t, e); report("synths: D -> E, prepared ahead", w);
            check(w.loaded == 0, "prepared ahead beside D's: nothing loaded");
            compare(t, e, "synths D -> E prepared ahead"); gone();
            // Too little room: F's twelve parts fill the free sounds of E's instance and spill to one more,
            // and back from F, E finds no instance with room for its sounds and loads one.
            w = load(t, f); report("synths: E -> F, too little room", w);
            check(w.reused >= 1 && w.loaded == 1, "too little room in E's instance: F spills to one more");
            compare(t, f, "synths E -> F (no room)"); gone();
            w = load(t, e); report("synths: F -> E, no room", w);
            check(w.reused == 0 && w.loaded == 1, "no instance with room: E spills to an instance of its own");
            compare(t, e, "synths F -> E (full)"); gone();
            // Other ports: a node whose saved note ports differ is refused as a fresh load is, leaving
            // no sound prepared beside what plays: D still fits after it.
            {
                const auto odd = temp.path / "odd"; fs::copy(d, odd, fs::copy_options::recursive);
                auto bytes = state_files::read(odd / "performance.json", 4 << 20); std::string text(bytes.begin(), bytes.end());
                const auto at = text.find("\"noteInputs\":1"); check(at != std::string::npos, "a synth's note inputs");
                text.replace(at, 14, "\"noteInputs\":2"); state_files::write(odd / "performance.json", text.data(), text.size());
                bool failed = false;
                try { t.loadPerformance(odd.string()); } catch(const std::exception& x) { failed = std::string(x.what()).find("note port layout changed") != std::string::npos; }
                check(failed, "other ports: refused as a fresh load is");
                for(int round = 0; round < 2; ++round) { w = load(t, round ? e : d); gone(); check(w.loaded == 0, "other ports: no sound left beside what plays"); }
            }
        }

        // The same Performance loaded again directly (issue 9): a silent synth holding the node's state is taken over, as
        // the Delay is (in a worker, kept with a reset), instead of its sound being prepared beside again (a Sampler loads
        // every sample again that way); one sounding still gets it beside and rings out, and is taken over the next time.
        {
            const auto again = temp.path / "again";
            ringing(again, 0, argv[1], argv[3], .5);
            Session t(48000, frames);
            load(t, again); quiet(t);
            w = load(t, again); report("again: J -> J, silent", w);
            check(w.reused == 2 && w.beside == 0 && w.loaded == 0 && w.kept >= 1, "loaded again, silent: the synth taken over, no sound beside: " + std::to_string(w.beside) + " beside");
            compare(t, again, "again: J -> J, silent"); quiet(t);
            t.note(true, 60, .8); blocks(t, 10);
            w = load(t, again); report("again: J -> J, a note held", w);
            // Issue 330: nothing changed it since it opened from this state, so the synth is kept as it is, sounding: no sound beside, no reset;
            // and its Delay with it (issue 340: kept at the silent reload before as well, so it holds its source still).
            check(w.beside == 0 && w.loaded == 0 && w.kept == 2 && w.open.beside == 0, "loaded again, a note held: the synth and its Delay kept as they are, nothing beside them: " + std::to_string(w.kept) + " kept");
            {
                Audio audio; bool rang = false; for(int i = 0; i < 20; ++i) { audio.run(t); rang = rang || audio.sounding(); }
                check(rang, "again: the held note plays on through the change");
            }
            t.note(false, 60, 0); quiet(t); quiet(t);
            w = load(t, again); report("again: J -> J, after sounds beside", w);
            check(w.reused == 2 && w.beside == 0 && w.loaded == 0, "after sounds beside, silent: the synth taken over");
            compare(t, again, "again: after sounds beside");
        }

        // The outgoing sounds ring out through the outgoing chain (RFC 0021, "Ringing out through the
        // outgoing chain"): G is a Basic Synth through a Delay at half level, H the same synth dry at
        // full level. A note held on G through the change to H: its tail must go through G's Delay and
        // level, the incoming sound through H's. The reference opens H from a copy of the synth, so
        // nothing is prepared beside: G's instance rings out on its own, as in a change between
        // Performances that share nothing. The two must sound the same, sample for sample.
        {
            const auto g = temp.path / "g", h = temp.path / "h", hcopy = temp.path / "h-copy";
            const auto synthCopy = temp.path / ("synth-copy" + fs::path(argv[1]).extension().string());
            fs::copy(argv[1], synthCopy, fs::copy_options::recursive);
            ringing(g, 0, argv[1], argv[3], .5); ringing(h, 1, argv[1], nullptr, 1); ringing(hcopy, 1, synthCopy.c_str(), nullptr, 1);
            Session t(48000, frames), r(48000, frames);
            load(t, g); load(r, g); quiet(t); quiet(r);
            t.note(true, 60, .8); r.note(true, 60, .8); blocks(t, 10); blocks(r, 10);
            const auto wt = load(t, h), wr = load(r, hcopy);
            report("ring out: G -> H, a note held", wt); report("ring out: G -> H's copy (reference)", wr);
            check(wt.beside == 1 && wt.loaded == 0, "G's synth given H's sound beside");
            check(wr.beside == 0 && wr.loaded == 1, "the reference loads H's copy");
            check(t.ringOutRoutes() == 2, "the held sound's ring-out routed into G's Delay: " + std::to_string(t.ringOutRoutes()) + " routes");
            {
                bool hidden = true;
                for(const auto& x : t.graphState().topology.audioRoutes) hidden = hidden && x.from.port != Graph::ringOutPort;
                check(hidden, "the ring-out routes are not in the public routing");
            }
            Audio x, y; bool same = true; double energy = 0, tail = 0, tms = 0, rms = 0;
            auto timed = [](Audio& a, Session& s, double& ms) { const auto at = std::chrono::steady_clock::now(); a.run(s); ms += std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - at).count(); };
            for(int block = 0; block < int(48000 * 5 / frames) && same; ++block)
            {
                if(block == 20) { t.note(false, 60, 0); r.note(false, 60, 0); }
                if(block == 30) { t.note(true, 72, .7); r.note(true, 72, .7); } // the incoming sound, through H's chain
                if(block == 60) { t.note(false, 72, 0); r.note(false, 72, 0); }
                if(block < 600) { timed(x, t, tms); timed(y, r, rms); } else { x.run(t); y.run(r); }
                if(x.l != y.l || x.r != y.r) { std::fprintf(stderr, "  ring out: different at block %d\n", block); same = false; }
                for(auto v : x.l) { energy += v * v; if(block >= 600) tail += v * v; }
            }
            check(same, "ring out: the outgoing tail through G's Delay and level, the incoming sound through H's, as in separate instances");
            check(energy > 1e-3, "ring out: audible");
            check(tail > 0, "ring out: G's Delay still echoing after both voices ended");
            quiet(t);
            {
                // The same notes once the outgoing chain is gone: H's chain alone.
                Audio z; double alone = 0;
                for(int block = 0; block < 600; ++block)
                {
                    if(block == 30) t.note(true, 72, .7);
                    if(block == 60) t.note(false, 72, 0);
                    timed(z, t, alone);
                }
                std::printf("ring out: render per block, the first 600 blocks: %.4f ms ringing out through G's chain, %.4f ms in separate instances, %.4f ms H alone\n", tms / 600, rms / 600, alone / 600);
                quiet(t);
            }
            check(t.ringOutRoutes() == 0, "the ring-out routes go once the outgoing sound is discarded: " + std::to_string(t.ringOutRoutes()));
            {
                bool gone = true;
                for(const auto& c : t.sounds(t.nodeId("synth")).contexts) gone = gone && c.name != "Ring 0";
                check(gone, "the outgoing sound discarded");
            }
            compare(t, h, "ring out: G -> H, then H as a fresh load");
            // Back from H to G with a note held: H has no effects, so its tail goes straight to the
            // outputs at H's level, and only retiring its sound removes those routes. The reference
            // plays H from the synth itself and opens G's copy.
            const auto gcopy = temp.path / "g-copy";
            ringing(gcopy, 0, synthCopy.c_str(), argv[3], .5);
            load(r, h); quiet(r); quiet(t);
            t.note(true, 60, .8); r.note(true, 60, .8); blocks(t, 10); blocks(r, 10);
            const auto bt = load(t, g), br = load(r, gcopy);
            check(bt.beside == 1 && br.beside == 0, "back to G: H's synth given G's sound beside; the reference opens G's copy");
            check(t.ringOutRoutes() == 2, "H's held sound rings out straight to the outputs: " + std::to_string(t.ringOutRoutes()) + " routes");
            {
                bool hidden = true;
                for(const auto& x : t.graphState().topology.audioRoutes) hidden = hidden && x.from.port != Graph::ringOutPort;
                check(hidden, "the ring-out routes to the outputs are not in the public routing");
            }
            same = true; energy = 0;
            for(int block = 0; block < 600 && same; ++block)
            {
                if(block == 20) { t.note(false, 60, 0); r.note(false, 60, 0); }
                if(block == 30) { t.note(true, 72, .7); r.note(true, 72, .7); }
                if(block == 60) { t.note(false, 72, 0); r.note(false, 72, 0); }
                x.run(t); y.run(r);
                if(x.l != y.l || x.r != y.r) { std::fprintf(stderr, "  ring out back: different at block %d\n", block); same = false; }
                for(auto v : x.l) energy += v * v;
            }
            check(same, "ring out: H's dry tail at H's level, G's sound through G's Delay, as in separate instances");
            check(energy > 1e-3, "ring out back: audible");
            quiet(t);
            check(t.ringOutRoutes() == 0, "the routes to the outputs go once H's sound is discarded: " + std::to_string(t.ringOutRoutes()));
        }

        // Only one change's tail carries over (RFC 0005, "Ringing out through the outgoing chain"): A is
        // a dark Basic Synth sound, dry at full level, B the synth through a Delay at half level, C the
        // synth dry at another level. A note held on A through the change to B rings out at A's level;
        // a second note is held on B; then the change to C. A's tail must fade out within the choke
        // time without a click, and B's go on through B's Delay. The reference plays B fresh and opens
        // C's copy: where A's tail has ended the two must sound the same, sample for sample, and before,
        // the difference is A's tail alone.
        {
            const auto a = temp.path / "older-a", b = temp.path / "older-b", c = temp.path / "older-c", ccopy = temp.path / "older-c-copy";
            const auto synthCopy = temp.path / ("synth-copy" + fs::path(argv[1]).extension().string());
            ringing(a, 3, argv[1], nullptr, 1, 0); ringing(b, 0, argv[1], argv[3], .5);
            ringing(c, 2, argv[1], nullptr, .8); ringing(ccopy, 2, synthCopy.c_str(), nullptr, .8);
            Session t(48000, frames), r(48000, frames);
            load(t, a); quiet(t);
            t.note(true, 60, .8); blocks(t, 10);
            check(load(t, b).beside == 1, "older tail: A's synth given B's sound beside");
            load(r, b);
            blocks(t, 5); blocks(r, 5);
            t.note(true, 72, .7); r.note(true, 72, .7);
            Audio x, y; float last = 0, peak = 0, slope = 0; // A's tail before the second change: t less r
            for(int block = 0; block < 10; ++block)
            {
                x.run(t); y.run(r);
                for(uint32_t i = 0; i < frames; ++i)
                {
                    const float d = x.l[i] - y.l[i];
                    if(block) slope = std::max(slope, std::abs(d - last));
                    peak = std::max(peak, std::abs(d)); last = d;
                }
            }
            check(peak > .01f, "older tail: A's tail sounds before the second change: " + std::to_string(peak));
            const auto wt = load(t, c), wr = load(r, ccopy);
            check(wt.beside == 1 && wr.loaded == 1, "older tail: B's synth given C's sound beside; the reference loads C's copy");
            check(t.ringOutRoutes() == 4, "older tail: B's sound rings out into B's Delay, A's to the outputs: " + std::to_string(t.ringOutRoutes()) + " routes");
            // A cut is never silent (the owner's rule): A's tail faded out by the change to C is said, and counted.
            bool said = false; for(const auto& n : t.sessionStatus().notices) said = said || n.find("from two Performances ago faded out") != std::string::npos;
            check(said && t.sessionStatus().ringOut.chokedOlder == 1, "older tail: the notice says A's notes faded out, and session.status counts it: " + std::to_string(t.sessionStatus().ringOut.chokedOlder));
            const uint32_t choke = 144; // the SDK's choke fade, 3 ms at 48 kHz
            float step = 0, after = 0; bool same = true; double energy = 0;
            for(int block = 0; block < 600; ++block)
            {
                if(block == 5) { t.note(false, 72, 0); r.note(false, 72, 0); }
                if(block == 10) { t.note(true, 64, .7); r.note(true, 64, .7); } // the incoming sound, through C's chain
                if(block == 40) { t.note(false, 64, 0); r.note(false, 64, 0); }
                if(block == 3) check(t.ringOutRoutes() == 2, "older tail: A's routes go once its sound has faded and is discarded: " + std::to_string(t.ringOutRoutes()));
                x.run(t); y.run(r);
                for(uint32_t i = 0; i < frames; ++i)
                {
                    const uint32_t at = uint32_t(block) * frames + i;
                    const float d = x.l[i] - y.l[i];
                    if(at <= choke) step = std::max(step, std::abs(d - last));
                    else { after = std::max(after, std::abs(d)); same = same && x.l[i] == y.l[i] && x.r[i] == y.r[i]; }
                    last = d;
                }
                for(auto v : x.l) energy += v * v;
            }
            std::printf("older tail: A's tail peak %.4f, its largest step %.5f before the second change, %.5f in the fade\n", peak, slope, step);
            check(after < 1e-6f, "older tail: A's tail gone within the choke time: " + std::to_string(after));
            check(step < 1.5f * slope, "older tail: faded without a click: a step of " + std::to_string(step) + " against " + std::to_string(slope));
            check(same, "older tail: B's tail through B's Delay and level, C's sound through C's, as in separate instances");
            check(energy > 1e-3, "older tail: audible");
            quiet(t);
            check(t.ringOutRoutes() == 0, "older tail: the ring-out routes go once B's sound is discarded: " + std::to_string(t.ringOutRoutes()));
        }

        // Graph::ringOut when one of the outgoing chain's plugins was taken over by the incoming
        // Performance: the synth plays into a Delay that rings out and into another plugin taken over
        // (not carried, added again under a new ID). The way into the Delay still rings out of the
        // ring-out buffer (two channels), the other is left out and counted; before, one such way
        // left none at all.
        {
            Graph g(48000, frames);
            const auto synth = g.add(argv[1]), fx = g.add(argv[3]), taken = g.add(argv[3]);
            g.connectAudio(synth, fx); g.connectAudio(fx, graphOutput); g.connectAudio(synth, taken); g.connectAudio(taken, graphOutput); g.commit();
            Graph next(48000, frames);
            next.carryOver(g, {fx});
            const auto now = next.add(g.node(synth).host); next.add(g.node(taken).host);
            const auto lost = next.ringOut(g, synth, now);
            check(next.ringOutRoutes(now) == 2 && lost == 2, "ring out: a way into a plugin taken over is left out, the others ring: " + std::to_string(next.ringOutRoutes(now)) + " routes, " + std::to_string(lost) + " left out");
        }

        // The owner's rule: at a Performance change, notes held by the keys or the pedal keep ringing,
        // never a cut, whatever the two Performances' chains after the instrument are. A note held by
        // the pedal through a change from A (the synth through an effect inserted after it, or sending
        // to one on a bus) to B: the same synth and effect plugins, both taken over (i); the same
        // synth and another effect, or none (ii); another synth (a copy, loaded afresh) and the same
        // effect (iii). The held note keeps sounding for 1.5 s, not gone in a block or two, and once it
        // has rung out B sounds as B opened fresh.
        {
            const auto synthCopy = temp.path / ("chain-synth" + fs::path(argv[1]).extension().string());
            fs::copy(argv[1], synthCopy, fs::copy_options::recursive);
            const Effect delay{argv[3]}, plain{argv[4], "org.kick.test.effect"}, none{};
            auto perf = [&](const std::string& name, const char* synth, Effect insert, Effect send, double brightness, double setting, bool pair = false, bool empty = false) {
                const auto f = temp.path / ("chain-" + name); chained(f, synth, insert, send, brightness, setting, pair, empty); return f;
            };
            const auto aEmpty = perf("a-empty", argv[1], plain, delay, .3, .6, false, true);
            struct Case { std::string what; fs::path a, b; bool master = false; };
            const auto aInsert = perf("a-insert", argv[1], delay, none, .3, .3), aPlain = perf("a-plain", argv[1], plain, none, .3, .6),
                       aSend = perf("a-send", argv[1], none, delay, .3, .3), aShared = perf("a-shared", argv[1], delay, delay, .3, .3, true);
            const std::vector<Case> cases{
                {"insert (i): same synth, same Delay", aInsert, perf("b-insert-same", argv[1], delay, none, .8, .5)},
                {"insert (i): same synth, same plain effect", aPlain, perf("b-plain-same", argv[1], plain, none, .8, .9)},
                {"insert (ii): same synth, the plain effect", aInsert, perf("b-insert-other", argv[1], plain, none, .8, .9)},
                {"insert (ii): same synth, dry", aInsert, perf("b-insert-dry", argv[1], none, none, .8, .5)},
                {"insert (iii): another synth, same Delay", aInsert, perf("b-insert-synth", synthCopy.c_str(), delay, none, .8, .5)},
                {"send (i): same synth, same Delay", aSend, perf("b-send-same", argv[1], none, delay, .8, .5)},
                {"send (ii): same synth, the plain effect", aSend, perf("b-send-other", argv[1], none, plain, .8, .9)},
                {"send (iii): another synth, same Delay", aSend, perf("b-send-synth", synthCopy.c_str(), none, delay, .8, .5)},
                {"shared (i): two parts on one synth, same Delays", aShared, perf("b-shared-same", argv[1], delay, delay, .8, .5, true)},
                {"shared (ii): two parts on one synth, the plain effect", aShared, perf("b-shared-other", argv[1], plain, plain, .8, .9, true)},
                {"shared (iii): another synth, same Delays", aShared, perf("b-shared-synth", synthCopy.c_str(), delay, delay, .8, .5, true)},
                {"empty bus: a level-0 send, another synth", aEmpty, perf("b-empty-synth", synthCopy.c_str(), none, delay, .8, .5), true},
                {"empty bus: a level-0 send, same synth", aEmpty, perf("b-empty-same", argv[1], plain, delay, .8, .9), true}};
            auto energy = [](const Audio& a) { double e = 0; for(auto v : a.l) e += double(v) * v; return e; };
            for(const auto& c : cases)
            {
                // r stays on A, the same notes: the note as it sounds without a change.
                Session t(48000, frames), r(48000, frames);
                for(auto* s : {&t, &r})
                {
                    load(*s, c.a); quiet(*s);
                    s->controller(64, 127); s->note(true, 60, .8); blocks(*s, 20); s->note(false, 60, 0); // held by the pedal
                }
                if(c.master)
                {
                    // The master lowered as the unit's switch test did (master.set, no graph.commit): it must be
                    // heard at once, a live gain, in both; with the level-0 send it only staged an edit.
                    Audio a, b; double was = 0, now = 0;
                    for(int i = 0; i < 2; ++i) { a.run(t); was += energy(a); b.run(r); }
                    for(auto* s : {&t, &r}) s->setMaster(.25, 0);
                    for(int i = 0; i < 2; ++i) { a.run(t); now += energy(a); b.run(r); }
                    check(now < .2 * was, "ring out, " + c.what + ": the master lowered is heard at once: " + std::to_string(was) + ", then " + std::to_string(now));
                }
                load(t, c.b);
                Audio x, y; double early = 0, late = 0, earlyA = 0, lateA = 0, dip = 1e9;
                for(int i = 0; i < 600; ++i)
                {
                    x.run(t); y.run(r);
                    const double e = energy(x), ea = energy(y);
                    if(i < 40) dip = std::min(dip, e / std::max(ea, 1e-30));
                    if(i < 10) { early += e; earlyA += ea; }
                    if(i >= 550 && i < 560) { late += e; lateA += ea; }
                }
                std::printf("ring out, %s: per 10 blocks, after the change %.4f then %.4f 1.5 s on; without it %.4f and %.4f; the least of a block's of the first 40 %.3f; %zu ring-out routes\n",
                            c.what.c_str(), early, late, earlyA, lateA, dip, t.ringOutRoutes());
                check(dip > .1 && late > .1 * lateA, "ring out, " + c.what + ": the pedaled note rings on through the change: the least block " + std::to_string(dip) + " of the note without it, 1.5 s on " + std::to_string(late) + " against " + std::to_string(lateA));
                t.controller(64, 0);
                for(int i = 0; i < int(48000 * 31 / frames) && t.leaving(); ++i) blocks(t, 1);
                quiet(t);
                check(!t.leaving(), "ring out, " + c.what + ": A's nodes gone once rung out");
                compare(t, c.b, ("ring out, " + c.what + ": then B as a fresh load").c_str());
            }
        }

        auto energyOf = [](const Audio& a) { double e = 0; for(auto v : a.l) e += double(v) * v; return e; };
        // Evidence (2) of 2026-11-19: a Sampler Performance, a chord held by the pedal, switched to one on another
        // instrument plugin (the unit's Nameless Piano -> Nameless saw: A's instances gone at +0.01 s). A's instance is not
        // taken over: it is kept, ringing through its own chain, takes no new notes, and goes once silent after the pedal
        // lets go. Without and with the converter's level-0 send to an empty bus (evidence 3).
        {
            { std::vector<float> wave(4 * 48000); for(size_t f = 0; f < wave.size(); ++f) wave[f] = float(.3 * ((f * 3) % 200) / 200.0 - .15); writeWavFile(samples / "long.wav", wave, 1, 48000); }
            const auto b = temp.path / "lower-saw"; lower(b, argv[1]);
            for(const bool empty : {false, true})
            {
                const std::string what = empty ? "a Sampler with a level-0 send to an empty bus" : "a Sampler";
                const auto a = temp.path / (empty ? "sampled-empty" : "sampled"); sampled(a, argv[2], argv[3], samples, empty);
                // t and w switch, t then striking a key A plays and B does not; r stays on A.
                Session t(48000, frames), w(48000, frames), r(48000, frames);
                for(auto* s : {&t, &w, &r})
                {
                    load(*s, a); quiet(*s); s->controller(64, 127);
                    for(const int k : {48, 60, 64, 67}) s->note(true, k, .8);
                    blocks(*s, 40);
                    for(const int k : {48, 60, 64, 67}) s->note(false, k, 0);
                    blocks(*s, 10);
                }
                load(t, b); load(w, b);
                Audio x, y, z; double dip = 1e9, late = 0, lateA = 0; bool same = true; size_t kept = 0;
                for(int i = 0; i < 400; ++i)
                {
                    if(i == 20) t.note(true, 72, .8);
                    if(i == 60) t.note(false, 72, 0);
                    x.run(t); y.run(w); z.run(r);
                    const double e = energyOf(x), ea = energyOf(z);
                    if(i < 40) dip = std::min(dip, e / std::max(ea, 1e-30));
                    if(i >= 370) { late += e; lateA += ea; } // a second on
                    same = same && x.l == y.l && x.r == y.r;
                    if(i == 200) kept = t.leaving();
                }
                std::printf("ring out, %s -> another instrument: the least block of the first 40 %.3f of the chord without the change, a second on %.4f against %.4f; %zu of A's nodes ringing out at 0.5 s\n",
                            what.c_str(), dip, late, lateA, kept);
                check(dip > .1 && late > .1 * lateA, "ring out, " + what + " -> another instrument: the pedaled chord rings on: the least block " + std::to_string(dip) + ", a second on " + std::to_string(late) + " against " + std::to_string(lateA));
                check(kept > 0, "ring out, " + what + " -> another instrument: A's instance is kept while it rings");
                check(same, "ring out, " + what + " -> another instrument: A's instance takes no new notes (a key only A plays sounds nothing)");
                t.controller(64, 0);
                for(int i = 0; i < int(48000 * 31 / frames) && t.leaving(); ++i) blocks(t, 1);
                check(!t.leaving(), "ring out, " + what + " -> another instrument: A's instance goes once silent after the pedal");
            }
        }
        // A send raised from 0 while a note sounds (a level-0 send is left out of the routes, so raising it compiles it
        // back): the send comes in without a click, and the part's fader and the master still move live after (no
        // graph.commit). To a bus with the Delay, and to the converter's empty bus (muted at a quarter level, so silent).
        for(const bool emptyBus : {false, true})
        {
            const std::string what = emptyBus ? "to an empty bus" : "to a bus with a Delay";
            Session t(48000, frames), r(48000, frames); uint32_t part = 0, bus = 0;
            for(auto* s : {&t, &r})
            {
                const auto n = s->add("synth", argv[1]);
                part = s->addPart("Part"); s->insertIntoPart(part, 0, n);
                bus = s->addPart(emptyBus ? "Reverb (not built yet)" : "Bus", {PartInput::Kind::Bus});
                if(emptyBus) s->setPartLevel(bus, .25, true);
                else { const auto fx = s->add("delay", argv[3]); s->insertIntoPart(bus, 0, fx); s->commit(); check(s->setParameter(fx, 1, .1) && s->setParameter(fx, 2, .3) && s->setParameter(fx, 3, .5), "delay settings"); }
                s->setSend(part, bus, 0); s->commit();
                check(s->selectSound(n, s->prepareSound(n, "Soft", {{1, .2}})), "select a sound");
                quiet(*s); s->note(true, 48, .8); blocks(*s, 40);
            }
            Audio x, y; float step = 0, steady = 0, prev = 0, prevR = 0; double after = 0;
            for(int i = 0; i < 80; ++i)
            {
                if(i == 40) { t.setSend(part, bus, .5); t.commit(); }
                x.run(t); y.run(r);
                for(uint32_t f = 0; f < frames; ++f)
                {
                    // The send's own sound, t less r: its largest step as it comes in against the note's own largest step.
                    const float d = x.l[f] - y.l[f];
                    if(i >= 40 && i < 44) step = std::max(step, std::abs(d - prev));
                    if(i < 40) steady = std::max(steady, std::abs(y.l[f] - prevR));
                    prev = d; prevR = y.l[f];
                }
                if(i >= 60) after += energyOf(x);
            }
            std::printf("send raised from 0, %s: its largest step coming in %.5f against the note's own %.5f\n", what.c_str(), step, steady);
            check(step <= steady, "send raised from 0, " + what + ": no click as it comes in: a step of " + std::to_string(step) + " against the note's " + std::to_string(steady));
            check(after > 0, "send raised from 0, " + what + ": the note still sounds");
            // To an empty bus that plays where the part does, the raised send and the part's fader are two routes, each live
            // (one route per set of live gains): both heard at once, nothing staged.
            Audio a; double was = 0, now = 0;
            for(int i = 0; i < 2; ++i) { a.run(t); was += energyOf(a); }
            t.setPartLevel(part, .25, false);
            for(int i = 0; i < 2; ++i) { a.run(t); now += energyOf(a); }
            check(now < .2 * was && !t.graphState().staged, "send raised from 0, " + what + ": the part's fader lowered is heard at once, nothing staged: " + std::to_string(was) + ", then " + std::to_string(now));
            t.setMaster(.25, 0);
            was = now; now = 0;
            for(int i = 0; i < 2; ++i) { a.run(t); now += energyOf(a); }
            check(now < .2 * was && !t.graphState().staged, "send raised from 0, " + what + ": the master lowered is heard at once, nothing staged: " + std::to_string(was) + ", then " + std::to_string(now));
        }
        // Master and part faders are live gains whatever the wiring (the owner's "# Organ", graph.get staged after master.set:
        // its parts send to the converter's empty bus at 0.0003 and 0.00025). A part sending to that empty bus (muted at a
        // quarter level) at 0.0003, at 0, or not at all, and the bus unmuted: master.set and the part's level heard in the next
        // block with no graph.commit, nothing staged; and the level heard then is the level a fresh load of the same mix gives.
        for(const double send : {.0003, 0., -1.})
            for(const bool unmuted : {false, true})
            {
                const std::string what = (send > 0 ? std::string("a 0.0003 send") : send == 0 ? std::string("a level-0 send") : std::string("no send")) + (unmuted ? " to an empty bus playing" : " to a muted empty bus");
                const auto file = temp.path / ("live-" + std::to_string(int(send * 1e4)) + (unmuted ? "-on" : "-off"));
                uint32_t part = 0, bus = 0;
                {
                    Session s(48000, frames);
                    const auto n = s.add("synth", argv[1]); part = s.addPart("Organ"); s.insertIntoPart(part, 0, n);
                    bus = s.addPart("Reverb (type 01 21, not built yet)", {PartInput::Kind::Bus}); s.setPartLevel(bus, .25, !unmuted);
                    if(send >= 0) s.setSend(part, bus, send);
                    s.setPartLevel(part, .86, false); s.setMaster(.497, 0); s.commit();
                    check(s.selectSound(n, s.prepareSound(n, "Organ", {{1, .5}})), "select a sound");
                    blocks(s, 2); s.savePerformance(file.string());
                }
                // t moves its levels; u, the same opened alike, does not: their energies' ratio, block for block, is the change.
                Session t(48000, frames), u(48000, frames);
                for(auto* s : {&t, &u}) { load(*s, file); quiet(*s); s->note(true, 48, .8); blocks(*s, 40); }
                check(!t.graphState().staged, what + ": nothing staged as it opens");
                Audio a, c;
                auto ratio = [&] { double mine = 0, still = 0; for(int i = 0; i < 4; ++i) { a.run(t); c.run(u); if(i >= 2) { mine += energyOf(a); still += energyOf(c); } } return mine / still; };
                const double before = ratio();
                t.setMaster(.12, 0);
                const double master = ratio() / before;
                t.setPartLevel(part, .43, false);
                const double both = ratio() / before;
                // Energies: the gains squared, .12/.497, then also .43/.86 (the send, the bus's level and the part's sum scale with the fader).
                const double m = std::pow(.12 / .497, 2);
                check(std::abs(master / m - 1) < 1e-3 && std::abs(both / (m * .25) - 1) < 1e-3 && !t.graphState().staged,
                      what + ": master.set and the part's level heard in the next block, no graph.commit, nothing staged: " + std::to_string(master / m) + ", " + std::to_string(both / (m * .25)));
                // The level then, against a fresh load of the mix with these levels: equal within float rounding.
                const auto moved = temp.path / ("live-moved-" + std::to_string(int(send * 1e4)) + (unmuted ? "-on" : "-off"));
                t.commit(); t.savePerformance(moved.string());
                Session r(48000, frames);
                load(r, moved); quiet(r); r.note(true, 48, .8); blocks(r, 52); // as many blocks after the note as t has played
                Audio b; double mine = 0, fresh = 0; for(int i = 0; i < 2; ++i) { a.run(t); b.run(r); mine += energyOf(a); fresh += energyOf(b); }
                check(std::abs(mine - fresh) <= 1e-6 * fresh, what + ": the level heard is a fresh load's: " + std::to_string(mine) + " against " + std::to_string(fresh));
            }

        ::setenv("OSCAIRN_AUDIO_CORES", "3", 1); // the FANTOM-07's: estimates and the lanes' total as on the unit
        // The ring-out fit rule (RFC 0021, 2026-11-19): a change rings out when the incoming Performance and the
        // outgoing one, priced by what still sounds, fit as one Performance does: their total within the budget on
        // every audio core, each chain within one block's. Measurements forgotten, every plugin counts at the same
        // guess, so the figures are the estimate's own. Before, both totals had to fit one block's budget: on the unit
        // a switch between two Sampler Performances (1.10 of a 0.75 block) cut, where opening ST Final (1.39) alone was
        // accepted.
        {
            const auto g1 = temp.path / "fit-a", g2 = temp.path / "fit-b";
            ringing(g1, 0, argv[1], argv[3], .5); ringing(g2, 1, argv[1], argv[3], .5);
            Session t(48000, frames), r(48000, frames);
            auto held = [&](Session& s, const fs::path& a) { load(s, a); quiet(s); s.controller(64, 127); s.note(true, 60, .8); blocks(s, 20); s.note(false, 60, 0); };
            held(t, g1); held(r, g1);
            t.setBudget({64}); t.forgetMeasurements();
            const auto open = t.estimate(""), next = t.estimate(g2.string());
            const auto cores = next.cores;
            if(cores < 2) std::printf("ring out fit: one audio core here, the rule's per-core case not checked\n");
            else
            {
                // A budget both fit together on the audio cores, not in one block.
                const double budget = std::max(next.longest, (open.cpu + next.cpu) / cores) * 1.1;
                check(open.cpu + next.cpu > budget, "ring out fit: the two together are past one block's budget");
                t.setBudget({budget}); t.forgetMeasurements();
                const auto e = t.estimate(g2.string());
                std::printf("ring out fit: the open Performance %.3f and the next %.3f of a budget of %.3f a block on %u cores: seamless %d\n", open.cpu, next.cpu, budget, cores, int(e.seamless));
                check(e.seamless && !e.fades, "ring out fit: two Performances that fit the audio cores together ring out");
                const auto routes = t.ringOutRoutes();
                load(t, g2);
                Audio x, y; double dip = 1e9;
                for(int i = 0; i < 40; ++i) { x.run(t); y.run(r); double a = 0, b = 0; for(auto v : x.l) a += double(v) * v; for(auto v : y.l) b += double(v) * v; dip = std::min(dip, a / std::max(b, 1e-30)); }
                check(t.ringOutRoutes() > routes && dip > .1, "ring out fit: the held note rings on through its Delay: " + std::to_string(dip));
                t.setBudget({});
            }
        }
        // Past the audio cores in CPU, with the memory there: the change no longer cuts. The last Performance is carried
        // over and its tail fades out in 150 ms (no step past a twentieth of the note's level), then its plugins go, said in a
        // notice. A: twice as many plain instruments as audio cores, one holding a note; B: a synth through a Delay.
        // With `stall`, the control thread ticks once in the fade and next well past its end (held up): the gain still
        // ramps to 0 across a block before A's plugins go, not a cut from where the last tick left it.
        // One audio core: with three, the incoming synth's chain sets the per-block budget, and beside it the outgoing instruments
        // fit in part, so issue 330's steps let some go and the rest ring on, never reaching the whole fade.
        ::setenv("OSCAIRN_AUDIO_CORES", "1", 1);
        for(const bool stall : {false, true})
        {
            const std::string how = stall ? "fade, the control thread held up: " : "fade: ";
            Session probe(48000, frames);
            const auto cores = probe.estimate("").cores;
            const auto heavy = temp.path / (stall ? "stall-a" : "fade-a"), light = temp.path / (stall ? "stall-b" : "fade-b");
            if(2 * cores <= 42)
            {
                many(heavy, 0, argv[4], 2 * cores); ringing(light, 1, argv[1], argv[3], .5);
                Session t(48000, frames);
                load(t, heavy); quiet(t);
                // A key held on every instrument: none of them is silent, so none goes first (issue 330), and beside even one it does not fit.
                t.controller(64, 127); for(uint32_t i = 0; i < 2 * cores; ++i) t.note(true, uint8_t(3 * i + 1), .8);
                blocks(t, 20);
                t.setBudget({64}); t.forgetMeasurements();
                const auto open = t.estimate(""), next = t.estimate(light.string());
                const double budget = std::max(next.longest, next.cpu / cores) * 1.05; // B alone fits; with A's total, not
                check(open.cpu + next.cpu > budget * cores, how + "the two together are past the audio cores");
                t.setBudget({budget}); t.forgetMeasurements();
                const auto e = t.estimate(light.string());
                check(e.fits && !e.seamless && e.fades, how + "B fits alone, not with A ringing out, and the memory is there: it fades");
                Audio x; float last = 0, before = 0, step = 0, after = 0;
                for(int i = 0; i < 10; ++i) { x.run(t); for(auto v : x.l) { before = std::max(before, std::abs(v)); last = v; } } // the held note's level
                load(t, light);
                bool said = false; for(const auto& n : t.sessionStatus().notices) said = said || n.find("faded out in 150 ms") != std::string::npos;
                check(said, how + "the notice says the last Performance's notes faded out");
                check(t.sessionStatus().ringOut.faded == 1 && t.sessionStatus().ringOut.cut == 0, how + "session.status counts the fade");
                const int fadeBlocks = int(.15 * 48000 / frames);
                double early = 0;
                // The control thread's tick as on a device, every 20 ms (8 blocks of 128): the fade's steps that far apart,
                // each ramped across a block.
                const int tick = stall ? fadeBlocks + 10 : int(.02 * 48000 / frames), quietFrom = stall ? fadeBlocks + 12 : fadeBlocks + 2 * tick + 2;
                for(int i = 0; i < fadeBlocks + 60; ++i)
                {
                    x.service = i % tick == 0;
                    x.run(t);
                    for(auto v : x.l) { step = std::max(step, std::abs(v - last)); last = v; if(i < 5) early += double(v) * v; if(i > quietFrom) after = std::max(after, std::abs(v)); }
                }
                x.service = true;
                std::printf("%slargest step %.5f against the note's level %.5f, %.6f left after the fade\n", how.c_str(), step, before, after);
                check(early > 0, how + "the tail still sounds as the fade begins (no cut)");
                check(before > 0 && step <= .05f * before, how + "no click, a step of " + std::to_string(step) + " against the level " + std::to_string(before));
                check(after == 0, how + "silent after the fade: " + std::to_string(after));
                blocks(t, 4);
                check(!t.leaving(), how + "A's plugins gone once faded");
                check(t.sessionStatus().ringOut.fadeEnded > 0 && t.sessionStatus().ringOut.limitEnded == 0, how + "session.status counts what the fade's end took: " + std::to_string(t.sessionStatus().ringOut.fadeEnded));
                for(uint32_t i = 0; i < 2 * cores; ++i) t.note(false, uint8_t(3 * i + 1), 0);
                t.controller(64, 0); t.setBudget({});
                compare(t, light, (how + "then B as a fresh load").c_str());
            }
            else std::printf("fade: %u audio cores, more parts than the fixture's keyboard holds: not checked\n", cores);
        }
        ::setenv("OSCAIRN_AUDIO_CORES", "3", 1);

        ::unsetenv("OSCAIRN_AUDIO_CORES");

        // A panic while edits are staged is taken then: kept for the next tick without them, it ended the tail of a change
        // made since (the change publishes, so unstages), silently. A: a synth through a Delay; panic with a key staged; a
        // pedaled note; the change to B: the note rings on.
        {
            const auto g1 = temp.path / "staged-a", g2 = temp.path / "staged-b";
            ringing(g1, 0, argv[1], argv[3], .5); ringing(g2, 1, argv[1], argv[3], .5);
            Session t(48000, frames);
            load(t, g1); quiet(t);
            t.setKey(MusicalKey{2, 0xAB5}); // staged, not committed
            t.allNotesOff(); blocks(t, 10);
            t.controller(64, 127); t.note(true, 60, .8); blocks(t, 20); t.note(false, 60, 0);
            load(t, g2);
            Audio x; double late = 0;
            for(int i = 0; i < 60; ++i) { x.run(t); if(i >= 50) for(const auto v : x.l) late += double(v) * v; }
            check(late > 0 && t.leaving() && t.sessionStatus().ringOut.panicEnded == 0, "a panic with edits staged ends no later change's tail: " + std::to_string(late) + ", " + std::to_string(t.leaving()) + " ringing");
            t.controller(64, 0);
        }

        if(const char* real = std::getenv("OSCAIRN_SWITCH_PERFORMANCES"); real && *real)
        {
            std::vector<fs::path> p; std::string all(real);
            for(size_t at = 0; at <= all.size();) { const auto bar = std::min(all.find('|', at), all.size()); p.emplace_back(all.substr(at, bar - at)); at = bar + 1; }
            if(p.size() != 3) throw std::runtime_error("OSCAIRN_SWITCH_PERFORMANCES names three folders, A|B|C");
            Session r(48000, frames);
            timeOnly(r, p);
        }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures) return 1;
    std::printf("ok: Performance switches measured\n");
    return 0;
}

