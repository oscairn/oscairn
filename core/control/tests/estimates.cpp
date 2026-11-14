// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0025's CPU estimates must not be spoiled by one stall or burst: a stall that sleeps (a plugin's
// thread not scheduled) does not move a plugin's cost at rest and one that computes moves it a
// little and wears off; notes held feed the cost per voice, not the rest; a file of impossible or
// the old estimator's figures is discarded; budget.forget lets go of the figures; a Performance
// refused for a spoiled figure opens once it is gone.
//   oscairn-estimates-test WORKER FIXTURE.clap VOICES.clap
#include <oscairn/json_rpc.h>
#include <oscairn/session.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <string>
#include <thread>
#include <unistd.h>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128;

    std::string read(const fs::path& file) { std::ifstream in(file); return {std::istreambuf_iterator<char>(in), {}}; }
    // A figure of the file's first plugin.
    double figure(const fs::path& home, const char* name)
    {
        const auto text = read(home / "measurements.json");
        const auto at = text.find(std::string("\"") + name + "\":");
        return at == std::string::npos ? -1 : std::strtod(text.c_str() + at + std::string(name).size() + 3, nullptr);
    }
    std::string keyOf(const fs::path& home)
    {
        const auto text = read(home / "measurements.json");
        const auto at = text.find("\"plugins\":{\"") + 12;
        return text.substr(at, text.find('"', at) - at);
    }
    void write(const fs::path& home, const std::string& text) { fs::create_directories(home); std::ofstream(home / "measurements.json") << text; }

    // A worker's effect played for `blocks` blocks, a service tick each; pace: a pause after each (a stall
    // is real time).
    struct Played
    {
        Session session{rate, frames};
        float l[frames]{}, r[frames]{}; float* out[2]{l, r};
        void play(int blocks, int pauseMicroseconds = 0)
        {
            for(int i = 0; i < blocks; ++i)
            {
                session.service(); session.process(out, frames);
                if(pauseMicroseconds) std::this_thread::sleep_for(std::chrono::microseconds(pauseMicroseconds));
            }
        }
    };

    // The stall is the plugin's call number 2500: after the worker's blocks of start-up (512 blocks) and
    // 2000 of steady ones. The rest before it, after it, and after many ordinary blocks.
    struct Stalled { double before, after, later, noise; }; // noise: the largest move of the rest between readings 100 blocks apart before the stall
    Stalled stall(const fs::path& home, const char* fixture, const char* spec)
    {
        ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
        ::setenv("KICK_FIXTURE_STALL", spec, 1);
        fs::create_directories(home);
        Played p;
        p.session.setBudget({.9, 0});
        const auto node = p.session.add("Effect", fixture, "org.kick.test.effect");
        p.session.connectAudio(node, graphOutput);
        p.session.commit();
        p.play(1950, 1000);
        p.session.saveMeasurementsNow();
        Stalled s{figure(home, "rest"), 0, 0, 0};
        for(int i = 0; i < 5; ++i)
        {
            p.play(100, 1000);
            p.session.saveMeasurementsNow();
            const auto now = figure(home, "rest");
            s.noise = std::max(s.noise, std::abs(now - s.before));
            s.before = now;
        }
        p.play(300, 1000); // the stall, and the blocks it silences
        p.play(300, 1000);
        p.session.saveMeasurementsNow();
        s.after = figure(home, "rest");
        p.play(20000);
        p.session.saveMeasurementsNow();
        s.later = figure(home, "rest");
        ::unsetenv("KICK_FIXTURE_STALL");
        return s;
    }
}

int main(int argc, char** argv)
{
    if(argc != 4) return 2;
    ::setenv("OSCAIRN_PLUGIN_WORKER", argv[1], 1);
    ::setenv("OSCAIRN_WORKER_BUDGET_US", "10000", 1);
    const auto root = fs::temp_directory_path() / ("oscairn-estimates-" + std::to_string(::getpid()));
    fs::remove_all(root); fs::create_directories(root);

    // Under a sanitizer the blocks are too slow and uneven to measure a share of one block against; the
    // logic below (notes, files, forgetting) is still checked.
#ifndef OSCAIRN_SANITIZED
    // A 100 ms sleep in a block: the thread was not running, the block's CPU time says so. Judged against
    // this run's own figures: the rest's noise over ordinary blocks, and the same stall made to compute.
    const auto asleep = stall(root / "asleep", argv[2], "2500:100000:0:0");
    std::printf("sleeping stall: rest %.6g before (noise %.3g), %.6g after, %.6g later\n", asleep.before, asleep.noise, asleep.after, asleep.later);
    check(asleep.before > 0, "the rest is measured before it: " + std::to_string(asleep.before));
    // 100 ms computing: a real cost, counted (at most a block's whole time), and gone again after ordinary blocks.
    const auto spun = stall(root / "spun", argv[2], "2500:100000:0:1");
    std::printf("spinning stall: rest %.6g before (noise %.3g), %.6g after, %.6g later\n", spun.before, spun.noise, spun.after, spun.later);
    const double noise = std::max(asleep.noise, spun.noise), moved = spun.after - spun.before;
    // A sample of one block weighs 1/n or more, so counting the stall's wall time would move the rest by about the
    // spinning stall's amount; it must move it by a small part of that, and by no more than the rest wanders.
    if(moved < 4 * noise) std::printf("too noisy here to tell the stalls apart (noise %.3g, the spinning stall moved it %.3g): not judged\n", noise, moved);
    else
    {
        check(moved > 2.5e-4 && moved < .01, "a stall that computes moves the rest by about one sample's share: " + std::to_string(moved));
        check(std::abs(asleep.after - asleep.before) < std::max(.25 * moved, 3 * noise), "a stall that sleeps does not move the rest: " + std::to_string(asleep.after - asleep.before) + " against " + std::to_string(moved));
        // Ordinary blocks wear the jump off: most of it is gone, and what is left is under half of it beside the rest's own drift.
        check(spun.after - spun.later > .7 * moved && spun.later - spun.before < .5 * moved + 3 * noise,
              "and it wears off over ordinary blocks: " + std::to_string(spun.before) + ", " + std::to_string(spun.after) + ", then " + std::to_string(spun.later));
    }
#endif

    // In the engine's process, with notes: held notes feed the cost per voice and the most notes, never the rest.
    ::unsetenv("OSCAIRN_PLUGIN_WORKER");
    ::setenv("OSCAIRN_FIXTURE_VOICE_US", "60", 1); // 60 us a note a block: 9% of a block for four
    const auto held = root / "held";
    ::setenv("OSCAIRN_DATA_HOME", held.c_str(), 1);
    fs::create_directories(held);
    double restBefore = 0, restAfter = 0;
    {
        Played p;
        p.session.setBudget({.9, 0});
        const auto node = p.session.add("Voices", argv[3], "org.oscairn.test.voices");
        const auto part = p.session.addPart("A"); p.session.insertIntoPart(part, 0, node); p.session.commit();
        p.play(700); // nothing played: 0.5 s quiet, then more than enough samples at rest
        p.session.saveMeasurementsNow();
        restBefore = figure(held, "rest");
        for(int key = 60; key < 64; ++key) p.session.note(true, key, 1);
        p.play(800);
        p.session.saveMeasurementsNow();
        restAfter = figure(held, "rest");
        std::printf("held notes: rest %.6g before, %.6g after; voice %.6g, voices %g\n", restBefore, restAfter, figure(held, "voice"), figure(held, "voices"));
        check(restBefore > 0 && std::abs(restAfter - restBefore) < std::max(2e-5, .05 * restBefore), "notes held do not change the rest: " + std::to_string(restBefore) + " then " + std::to_string(restAfter));
        check(figure(held, "voices") == 4 && figure(held, "voice") > .015 && figure(held, "voice") < .04, "they give the cost per voice (60 us of 2667 us), for the voices seen");
    }
    check(read(held / "measurements.json").find("\"version\":4") != std::string::npos, "the file is versioned");
    const auto key = keyOf(held);

    // Files that are not trusted: the old estimator's (no version), impossible figures (a plugin at
    // rest above all of the audio time, negative), each measured again.
    auto estimateWith = [&](const std::string& name, const std::string& text, double budget = .9) {
        const auto home = root / name;
        write(home, text);
        ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
        Session s(rate, frames);
        s.setBudget({budget, 0});
        const auto node = s.add("a", argv[3], "org.oscairn.test.voices");
        const auto part = s.addPart("A"); s.insertIntoPart(part, 0, node); s.commit();
        return s.estimate().plugins.at(0);
    };
    const auto entry = [&](const std::string& body) { return "\"" + key + "\":{" + body + "}"; };
    const auto old = estimateWith("old", "{\"plugins\":{" + entry("\"cpu\":2.11518,\"rest\":2.11518,\"voice\":0,\"voices\":2,\"memory\":68947968") + "}}");
    check(!old.measured && std::abs(old.cpu - .02) < 1e-9, "a file of the old estimator is discarded: " + std::to_string(old.cpu));
    const auto impossible = estimateWith("impossible", "{\"version\":4,\"plugins\":{" + entry("\"cpu\":2.11518,\"rest\":2.11518,\"voice\":0,\"voices\":2,\"memory\":68947968") + "}}");
    check(std::abs(impossible.cpu - .02) < 1e-9 && impossible.memory >= 68947968, "a rest above the audio time is dropped, its memory kept: " + std::to_string(impossible.cpu));
    const auto negative = estimateWith("negative", "{\"version\":4,\"plugins\":{" + entry("\"cpu\":0.1,\"rest\":-0.1,\"voice\":0,\"voices\":0,\"memory\":1000,\"samples\":1000") + "}}");
    check(std::abs(negative.cpu - .02) < 1e-9, "a negative one is dropped: " + std::to_string(negative.cpu));
    const auto sound = estimateWith("sound", "{\"version\":4,\"plugins\":{" + entry("\"cpu\":0.3,\"rest\":0.05,\"voice\":0.01,\"voices\":3,\"memory\":1000,\"samples\":1000") + "}}");
    check(sound.measured && std::abs(sound.cpu - .3) < 1e-9, "a possible one stands: " + std::to_string(sound.cpu));

    // A Performance refused for a figure that is possible but spoiled (70% where a plugin costs 1%), over
    // a 50% budget, opens once the figures are forgotten (budget.forget); one with no figures at all is allowed.
    {
        const auto home = root / "refused";
        write(home, "{\"version\":4,\"plugins\":{" + entry("\"cpu\":0.7,\"rest\":0.7,\"voice\":0,\"voices\":0,\"memory\":1000,\"samples\":1000") + "}}");
        ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
        const auto saved = root / "performance";
        {
            Session s(rate, frames);
            const auto node = s.add("a", argv[3], "org.oscairn.test.voices");
            const auto part = s.addPart("A"); s.insertIntoPart(part, 0, node); s.commit();
            s.savePerformance(saved.string());
        }
        Session s(rate, frames);
        s.setBudget({.5, 0});
        std::string why;
        try { s.loadPerformance(saved.string()); } catch(const std::exception& e) { why = e.what(); }
        check(why.find("does not fit") != std::string::npos || why.find("of the audio time") != std::string::npos, "refused for the spoiled figure: " + why);
        check(why.find("most:") != std::string::npos && why.find("70%") != std::string::npos, "saying which part costs most: " + why);
        check(!s.estimate(saved.string()).fits, "and the library would say it does not fit");
        JsonRpc rpc(s);
        const auto reply = rpc.dispatch(R"({"jsonrpc":"2.0","id":1,"method":"budget.forget","params":{}})").value_or("");
        check(reply.find("\"forgotten\":true") != std::string::npos, "budget.forget answers: " + reply);
        const auto e = s.estimate(saved.string());
        check(e.fits, "it fits once forgotten");
        check(std::abs(e.plugins.at(0).cpu - .02) < 1e-9, "at the default estimate: " + std::to_string(e.plugins.at(0).cpu));
        try { s.loadPerformance(saved.string()); why.clear(); } catch(const std::exception& x) { why = x.what(); }
        check(why.empty(), "and it opens: " + why);
        s.saveMeasurementsNow();
        check(figure(home, "rest") == 0 && figure(home, "memory") == 1000, "the file keeps the memory and no CPU");
    }
    fs::remove_all(root);
    return failures ? 1 : 0;
}
