// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0025: a plugin's recorded cost is its own CPU time per block as a share of the block period. The
// voice fixture in a worker spends a known CPU time every block and a known time per sounding voice
// (OSCAIRN_FIXTURE_CPU_CLOCK: on the thread's CPU clock, so the cost is exact however the thread is
// scheduled). Played at rest, with notes held, and at rest again right after its notes' 1 s release
// tails: the recorded rest and cost per voice are those times, within a tolerance, alone, with the extra
// block, and with every core kept busy by other threads (the worker sharing a core). A figure from these
// few seconds of blocks is not yet certain, so admission does not refuse on it alone.
//   oscairn-measured-costs-test WORKER VOICES.clap
#include <oscairn/session.h>
#include "../../plugin-host/src/StateFiles.h"
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <string>
#include <thread>
#include <vector>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    constexpr double period = frames / rate * 1e6; // 2,667 us
    constexpr double blockUs = 300, voiceUs = 200; // the fixture's costs
    constexpr int notes = 6;

    std::string read(const fs::path& file) { std::ifstream in(file); return {std::istreambuf_iterator<char>(in), {}}; }
    double figure(const fs::path& home, const char* name)
    {
        const auto text = read(home / "measurements.json");
        const auto at = text.find(std::string("\"") + name + "\":");
        return at == std::string::npos ? -1 : std::strtod(text.c_str() + at + std::string(name).size() + 3, nullptr);
    }

    void measure(const fs::path& home, const char* fixture, const char* what, bool extraBlock, bool busyCores)
    {
        ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
        if(extraBlock) ::setenv("OSCAIRN_WORKER_EXTRA_BLOCK", "org.oscairn.test.voices", 1);
        else ::unsetenv("OSCAIRN_WORKER_EXTRA_BLOCK");
        fs::create_directories(home);
        std::atomic<bool> stop{false};
        std::vector<std::thread> spinners;
        if(busyCores) for(unsigned i = 0; i < std::max(1u, std::thread::hardware_concurrency()); ++i) spinners.emplace_back([&] { while(!stop.load(std::memory_order_relaxed)) {} });
        {
            Session s(rate, frames);
            s.setBudget({.9, 0});
            const auto node = s.add("Voices", fixture, "org.oscairn.test.voices");
            PartInput keys; keys.zones = {NoteZone{0, 127}};
            s.insertIntoPart(s.addPart("Part", keys), 0, node); s.commit();
            float l[frames]{}, r[frames]{}; float* out[2]{l, r};
            auto play = [&](int blocks, std::vector<kick::clap::Event> events = {}) {
                for(int i = 0; i < blocks; ++i) { s.service(); s.process(out, frames, i ? std::span<const kick::clap::Event>() : std::span<const kick::clap::Event>(events)); }
            };
            std::vector<kick::clap::Event> on, off;
            for(int k = 0; k < notes; ++k) { on.push_back(kick::clap::Event::note(true, 60 + k, .8)); off.push_back(kick::clap::Event::note(false, 60 + k, 0)); }
            play(1500);      // past the load's first blocks, then at rest
            play(1500, on);  // notes held
            play(1500, off); // their release tails (1 s), then at rest again
            s.saveMeasurementsNow();
            const auto e = s.estimate();
            check(e.plugins.size() == 1 && !e.plugins[0].certain, std::string(what) + ": a few seconds of blocks is not yet a certain figure");
        }
        stop = true;
        for(auto& t : spinners) t.join();
        const double rest = figure(home, "rest") * period, voice = figure(home, "voice") * period;
        std::printf("%s: rest %.0f us (spends %.0f), per voice %.0f us (spends %.0f)\n", what, rest, blockUs, voice, voiceUs);
        // Within a tenth and the worker's own few microseconds a block: a tenfold error is far outside, and so is
        // the rest taken during the release tails (a third more here) or the wall time of a thread sharing a core.
        // With every core busy, within two fifths: in a virtual machine (the Linux container on a Mac) the time
        // its host takes a virtual core away counts as the guest thread's CPU time (seen: a third more).
        const double within = busyCores ? .4 : .1;
        auto near = [&](double measured, double spent) { return std::abs(measured - spent) <= within * spent + 15; };
        check(near(rest, blockUs), std::string(what) + ": the rest is the block's own CPU time: " + std::to_string(rest));
        check(near(voice, voiceUs), std::string(what) + ": the cost per voice is a voice's CPU time: " + std::to_string(voice));
    }
}

int main(int argc, char** argv)
{
    if(argc != 3) return 2;
    ::setenv("OSCAIRN_PLUGIN_WORKER", argv[1], 1);
    ::setenv("OSCAIRN_FIXTURE_CPU_CLOCK", "1", 1);
    ::setenv("OSCAIRN_FIXTURE_BLOCK_US", std::to_string(blockUs).c_str(), 1);
    ::setenv("OSCAIRN_FIXTURE_VOICE_US", std::to_string(voiceUs).c_str(), 1);
    ::setenv("OSCAIRN_WORKER_BUDGET_US", "50000", 1); // unpaced: no block is given up
    state_files::TemporaryDirectory temp;
    measure(temp.path / "alone", argv[2], "alone", false, false);
    measure(temp.path / "extra", argv[2], "with the extra block", true, false);
    measure(temp.path / "busy", argv[2], "every core busy", false, true);
    if(failures) { std::fprintf(stderr, "%d failures\n", failures); return 1; }
    std::puts("ok: a recorded cost is the plugin's own CPU time per block");
    return 0;
}
