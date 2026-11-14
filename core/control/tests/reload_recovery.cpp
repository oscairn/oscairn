// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A reload while playing whose new instances are slow in their first blocks (on the FANTOM-07, a
// Performance reopened with another app open: three parts failed as too slow, restarted three
// times in a minute and given up, then every later load refused until the bundle was restarted).
// Three parts of the voice fixture in workers, keys held, the audio thread rendering in real time
// with the budget set as on the device; the Performance is opened again with each new instance's
// first 400 blocks taking 3 ms more than the 1.33 ms allowance. Lateness while loading is not "too
// slow": no worker is restarted or failed. Nor is it the plugin's cost: its figures, certain from
// earlier playing, stay as they were (measured from the storm: 0.58 of a block from 0.002, which
// refuses every Performance using the plugin once its chain passes the budget's share, and is never
// measured down while it is refused). Then an empty Performance and the first again open.
//   oscairn-reload-recovery-test WORKER VOICES.clap
#include "oscairn/session.h"
#include "../../plugin-host/src/StateFiles.h"
#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <regex>
#include <string>
#include <thread>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    const char* fixture = nullptr;

    void save(const fs::path& to, int parts)
    {
        Session s(rate, frames);
        for(int i = 0; i < parts; ++i)
        {
            const auto part = s.addPart("Part " + std::to_string(i));
            s.insertIntoPart(part, 0, s.add("Voices " + std::to_string(i), fixture, "org.oscairn.test.voices"));
        }
        s.commit();
        s.savePerformance(to.string());
    }
    // Played a few seconds offline and saved; its figures then marked certain, as a plugin played for a while on the device.
    void seed(const fs::path& home, const fs::path& performance)
    {
        {
            Session w(rate, frames);
            w.setBudget({.9, 0});
            w.loadPerformance(performance.string());
            float l[frames]{}, r[frames]{}; float* out[2]{l, r};
            auto play = [&](int blocks) { for(int i = 0; i < blocks; ++i) { w.service(); w.process(out, frames); } };
            play(1500);
            for(int key = 60; key < 63; ++key) w.note(true, key, .8);
            play(1500);
            for(int key = 60; key < 63; ++key) w.note(false, key, 0);
            play(1500);
            w.saveMeasurementsNow();
        }
        const auto file = home / "measurements.json";
        std::ifstream in(file); std::string text{std::istreambuf_iterator<char>(in), {}}; in.close();
        std::ofstream(file) << std::regex_replace(text, std::regex("\"samples\":[0-9]+"), "\"samples\":20000");
    }
    std::string why(const Session& s) { std::string all; for(const auto& f : s.sessionStatus().failures) all += f.name + ": " + f.reason + "; "; return all; }
}

int main(int argc, char** argv)
{
    if(argc != 3) { std::fprintf(stderr, "usage: %s WORKER VOICES.clap\n", argv[0]); return 2; }
    ::setenv("OSCAIRN_PLUGIN_WORKER", argv[1], 1);
    fixture = argv[2];
    state_files::TemporaryDirectory home, temp;
    ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
    const auto three = temp.path / "three", empty = temp.path / "empty";
    save(three, 3); save(empty, 0);
    seed(home.path, three);
    double seeded = 0;
    { Session e(rate, frames); e.setBudget({.9, 0}); for(const auto& c : e.estimate(three.string()).plugins) seeded = std::max(seeded, c.cpu); }
    std::printf("seeded: %.3f of a block\n", seeded);

    Session s(rate, frames);
    s.setBudget({.9, 0});
    s.loadPerformance(three.string());
    std::atomic<bool> stop{false};
    std::thread audio([&] {
        std::array<float, frames> l{}, r{}; float* out[2]{l.data(), r.data()};
        auto next = std::chrono::steady_clock::now();
        while(!stop)
        {
            s.process(out, frames);
            next += std::chrono::microseconds(int64_t(1e6 * frames / rate));
            std::this_thread::sleep_until(next);
        }
    });
    auto serve = [&](double seconds) {
        for(const auto until = std::chrono::steady_clock::now() + std::chrono::duration<double>(seconds); std::chrono::steady_clock::now() < until;)
        { s.service(); std::this_thread::sleep_for(std::chrono::milliseconds(5)); }
    };
    std::string refused;
    try
    {
        serve(.5);
        for(int key = 60; key < 63; ++key) s.note(true, key, .8);
        serve(.5);
        ::setenv("OSCAIRN_FIXTURE_SPIKE", "1:400:3000", 1); // each instance loaded from here: its first 400 blocks 3 ms late
        s.loadPerformance(three.string());
        serve(4); // past the slow blocks; without the exemption, three restarts and given up by now
        const auto failed = why(s);
        check(failed.empty(), "a reload's slow first blocks fail no worker: " + failed);
        // Nor are they recorded as the plugin's cost, which would refuse the Performance from then on.
        double most = 0;
        for(const auto& c : s.estimate(three.string()).plugins) most = std::max(most, c.cpu);
        { const auto e = s.estimate(three.string()); std::printf("after the reload: fits %d, plugins:", int(e.fits)); for(const auto& c : e.plugins) std::printf(" %s %.3f%s", c.name.c_str(), c.cpu, c.certain ? " certain" : ""); std::printf("\n"); }
        check(most < 2 * seeded + .02, "its load's blocks are not its cost: " + std::to_string(most) + " of a block, " + std::to_string(seeded) + " before");
        ::unsetenv("OSCAIRN_FIXTURE_SPIKE");
        try { s.loadPerformance(empty.string()); } catch(const std::exception& e) { refused = std::string("the empty one: ") + e.what(); }
        serve(.5);
        try { s.loadPerformance(three.string()); } catch(const std::exception& e) { refused += std::string(" the first: ") + e.what(); }
        serve(1);
        check(why(s).empty(), "and nothing failed after: " + why(s));
    }
    catch(const std::exception& e) { check(false, e.what()); }
    stop = true; audio.join();
    check(refused.empty(), "an empty Performance and the first again open after it: " + refused);
    if(!failures) std::printf("ok: a reload's slow first blocks restart nothing, and the engine opens Performances after\n");
    return failures ? 1 : 0;
}
