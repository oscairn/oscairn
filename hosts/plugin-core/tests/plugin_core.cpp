// SPDX-License-Identifier: AGPL-3.0-or-later
// A test host for the DAW plugin's core (RFC 0047 step 1, #293): it creates an instance, lists the library,
// picks a Performance and plays it at the block sizes DAWs use, switches while playing, saves the state,
// destroys the instance and restores it in a new one, and restores states naming a plugin that is not
// installed and a Performance that does not open. Every block is played under the allocation guard.
//
//   oscairn-plugin-core-test basic-synth.clap delay.clap [oscairn-plugin-worker]
#include "oscairn/plugin_core.h"
#include "oscairn/session.h"
#include "AllocationGuard.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>
#include <unistd.h>

namespace fs = std::filesystem;
using namespace oscairn::plugin;

namespace
{
    constexpr double rate = 48000;
    constexpr uint32_t maxFrames = 512;
    int failures = 0;

    void check(bool ok, const std::string& what)
    {
        if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; }
    }

    struct Audio
    {
        std::vector<float> left = std::vector<float>(maxFrames), right = std::vector<float>(maxFrames);
    };

    // One block under the allocation guard: its peak, or -1 when it did not play.
    float block(Instance& instance, Audio& audio, uint32_t frames, std::vector<MidiEvent> events = {})
    {
        Block b;
        b.frames = frames;
        b.events = events.data();
        b.eventCount = uint32_t(events.size());
        b.main[0] = audio.left.data();
        b.main[1] = audio.right.data();
        forbidAllocation = true;
        const bool ok = instance.process(b);
        forbidAllocation = false;
        if(!ok) return -1;
        float peak = 0;
        for(uint32_t i = 0; i < frames; ++i) peak = std::max({peak, std::abs(audio.left[i]), std::abs(audio.right[i])});
        return peak;
    }

    MidiEvent note(bool on, uint8_t key, uint32_t frame = 0) { return {frame, 3, {uint8_t(on ? 0x90 : 0x80), key, uint8_t(on ? 100 : 0)}}; }

    // A note played over blocks of these sizes, struck in the first block (at its last frame) and let go after:
    // the loudest the blocks were.
    float play(Instance& instance, const std::vector<uint32_t>& sizes, uint8_t key = 60)
    {
        Audio audio;
        float peak = 0;
        bool played = true;
        for(size_t i = 0; i < sizes.size(); ++i)
        {
            const auto p = block(instance, audio, sizes[i], i ? std::vector<MidiEvent>{} : std::vector<MidiEvent>{note(true, key, sizes[i] - 1)});
            played = played && p >= 0;
            peak = std::max(peak, p);
        }
        check(played, "every block of the sequence plays");
        for(int i = 0; i < 100; ++i) block(instance, audio, 480, i ? std::vector<MidiEvent>{} : std::vector<MidiEvent>{note(false, key)}); // its release rings out
        return peak;
    }

    // Blocks played as a DAW goes on while the instance loads, and ten after: whether it became ready, and the
    // loudest block from the start.
    bool settle(Instance& instance, float* loudest = nullptr)
    {
        Audio audio;
        const auto until = std::chrono::steady_clock::now() + std::chrono::seconds(60);
        bool played = true;
        for(int after = 0; after < 10 && std::chrono::steady_clock::now() < until;)
        {
            if(instance.status().load != Load::Loading) ++after;
            const auto p = block(instance, audio, 128);
            played = played && p >= 0;
            if(loudest) *loudest = std::max(*loudest, p);
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }
        check(played, "audio goes on while a Performance loads");
        return instance.status().load == Load::Ready;
    }

    std::string read(const fs::path& file) { std::ifstream in(file, std::ios::binary); return {std::istreambuf_iterator<char>(in), {}}; }

    bool mentions(const std::vector<std::string>& list, const std::string& text)
    {
        return std::any_of(list.begin(), list.end(), [&](const std::string& s) { return s.find(text) != std::string::npos; });
    }

    // The worker processes this test started.
    int workers()
    {
        const auto command = "pgrep -P " + std::to_string(getpid()) + " oscairn-plugin";
        FILE* p = popen(command.c_str(), "r");
        if(!p) return -1;
        int n = 0;
        for(int c; (c = std::fgetc(p)) != EOF;) n += c == '\n';
        pclose(p);
        return n;
    }

    constexpr const char* notInstalled = "org.example.not-installed";
}

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: oscairn-plugin-core-test basic-synth.clap delay.clap [oscairn-plugin-worker]\n"); return 2; }
    const std::string synthPath = argv[1], delayPath = argv[2], worker = argc > 3 ? argv[3] : "";
    const auto home = fs::temp_directory_path() / ("oscairn-plugin-core-test-" + std::to_string(getpid()));
    fs::remove_all(home);
    fs::create_directories(home);
    setenv("OSCAIRN_DATA_HOME", home.c_str(), 1); // the library the test builds, not the user's

    // The library: the Basic Synth alone, the Basic Synth into the Delay, and a Performance whose synth is a
    // plugin installed nowhere, as a project from another machine would name it.
    std::string synthOnly, withDelay, missing;
    {
        oscairn::Session s(rate, maxFrames);
        const auto synth = s.add("synth", synthPath);
        const auto part = s.addPart("Synth");
        s.insertIntoPart(part, 0, synth);
        s.commit();
        synthOnly = s.savePerformanceToLibrary("DAW Synth", true);
        missing = s.savePerformanceToLibrary("DAW Missing", true);
        s.insertIntoPart(part, 1, s.add("delay", delayPath));
        s.commit();
        withDelay = s.savePerformanceToLibrary("DAW Synth and Delay", true);

        oscairn::Control::LibraryQuery q;
        q.id = missing;
        fs::path document = s.libraryQuery(q).items.at(0).path;
        if(fs::is_directory(document)) document /= "performance.json";
        auto text = read(document);
        auto replace = [&](const std::string& before) {
            const auto at = text.find(before);
            check(at != std::string::npos, "the saved document holds " + before);
            if(at == std::string::npos) return;
            const auto from = at + before.size();
            text.replace(from, text.find('"', from) - from, before.find("pathHint") != std::string::npos ? (home / "gone.clap").string() : notInstalled);
        };
        replace("\"plugin\":{\"format\":\"clap\",\"id\":\"");
        replace("\"pathHint\":\"");
        std::ofstream(document, std::ios::binary | std::ios::trunc) << text;
        s.libraryRescan();
    }

    Config config;
    config.sampleRate = rate;
    config.maxFrames = maxFrames;
    config.bundledPlugins = fs::path(synthPath).parent_path().string();
    config.worker = worker;

    std::vector<uint8_t> saved;
    {
        Instance instance(config);
        instance.activate(rate);
        const auto items = instance.library();
        auto listed = [&](const std::string& id, const std::string& name) {
            return std::any_of(items.begin(), items.end(), [&](const Item& i) { return i.id == id && i.name == name; });
        };
        check(listed(synthOnly, "DAW Synth") && listed(withDelay, "DAW Synth and Delay") && listed(missing, "DAW Missing"), "the library lists the installed Performances by ID and name");
        check(instance.status().load == Load::None && instance.status().performance.id.empty(), "a new instance has picked nothing");
        check(play(instance, {128, 128, 128}) == 0, "with nothing picked it plays silence");

        Audio audio;
        check(block(instance, audio, 0) < 0 && block(instance, audio, maxFrames + 1) < 0, "a block of no frames, or past the maximum, is refused");

        instance.pick(synthOnly);
        check(instance.status().load == Load::Loading, "a pick returns at once, loading");
        check(settle(instance), "the picked Performance loads: " + instance.status().error);
        auto status = instance.status();
        check(status.performance.id == synthOnly && status.performance.name == "DAW Synth" && status.missing.empty(), "its identity, nothing missing");
        for(const auto& sizes : std::vector<std::vector<uint32_t>>{std::vector<uint32_t>(4800, 1), std::vector<uint32_t>(75, 64), std::vector<uint32_t>(38, 128),
                                                                   std::vector<uint32_t>(10, 480), {1, 7, 512, 33, 480, 64, 2, 300, 128, 511, 17, 256, 1, 90, 512}})
            check(play(instance, sizes) > 0.01f, "notes sound at blocks of " + std::to_string(sizes[0]) + (sizes.size() > 1 && sizes[1] != sizes[0] ? " and varying" : ""));
        if(!worker.empty()) check(workers() > 0, "its plugin runs in a worker process");

        // A switch while a note is held: the Performance before it plays on until the new one is ready.
        block(instance, audio, 128, {note(true, 64)});
        instance.pick(withDelay);
        float during = 0;
        check(settle(instance, &during), "the second Performance loads: " + instance.status().error);
        check(during > 0.01f, "the held note of the first sounds on through the switch");
        check(instance.status().performance.id == withDelay, "the second plays");
        check(play(instance, {480, 480, 480, 480}, 67) > 0.01f, "the second sounds");

        instance.pick("no-such-performance");
        check(!settle(instance) && instance.status().load == Load::Failed && !instance.status().error.empty(), "a pick of nothing in the library fails, saying why");
        check(instance.status().performance.id == withDelay && play(instance, {480, 480}) > 0.01f, "and the Performance before it plays on");

        saved = instance.state();
        check(saved.size() > 8, "the state holds the Performance");
        instance.deactivate();
    }

    {
        Instance instance(config);
        instance.activate(rate);
        instance.setState(saved);
        check(settle(instance), "the saved state opens in a new instance: " + instance.status().error);
        const auto status = instance.status();
        check(status.performance.id == withDelay && status.performance.name == "DAW Synth and Delay" && status.missing.empty(), "the same Performance, by identity");
        check(play(instance, {64, 480, 1, 128, 512, 300}) > 0.01f, "and it plays");

        bool refused = false;
        try { instance.setState({1, 2, 3}); }
        catch(const std::invalid_argument&) { refused = true; }
        check(refused && instance.status().load == Load::Ready && instance.status().performance.id == withDelay, "bytes that are no state are refused, changing nothing");
        auto newer = saved;
        newer[4] = 2;
        refused = false;
        try { instance.setState(newer); }
        catch(const std::invalid_argument& e) { refused = std::string(e.what()).find("version 2") != std::string::npos; }
        check(refused, "a state of a later version is refused, saying so");

        // A Performance that does not open (here: a format version this build does not read) leaves the one
        // before it playing, and the state is given back as it came.
        auto broken = saved;
        const std::string from = "\"version\":13", to = "\"version\":99";
        const auto at = std::search(broken.begin(), broken.end(), from.begin(), from.end());
        check(at != broken.end(), "the state holds the Performance's document");
        if(at != broken.end()) std::copy(to.begin(), to.end(), at);
        instance.setState(broken);
        check(!settle(instance) && instance.status().error.find("unsupported format version") != std::string::npos, "a state whose Performance does not open fails, saying why");
        check(instance.status().performance.id == withDelay && play(instance, {480, 480}) > 0.01f, "the Performance before it plays on");
        check(instance.state() == broken, "and the project's state is kept as it came");
        instance.setState(saved);
        check(settle(instance) && instance.state() != broken, "a state that opens replaces it");
    }

    std::vector<uint8_t> withMissing;
    {
        Instance instance(config);
        instance.activate(rate);
        instance.pick(missing);
        check(settle(instance), "a Performance naming a plugin that is not here opens: " + instance.status().error);
        check(mentions(instance.status().missing, notInstalled), "and says the plugin is missing");
        withMissing = instance.state();
    }
    {
        Instance instance(config);
        instance.activate(rate);
        instance.setState(withMissing);
        check(settle(instance), "its state restores: " + instance.status().error);
        check(instance.status().performance.id == missing && mentions(instance.status().missing, notInstalled), "reporting the missing plugin again");
        check(play(instance, {480, 480}) >= 0, "playing on without it");
        const auto again = instance.state();
        Instance next(config);
        next.activate(rate);
        next.setState(again);
        check(settle(next) && mentions(next.status().missing, notInstalled), "saved again, the stand-in keeps the missing plugin's place");
    }

    fs::remove_all(home);
    if(failures) { std::fprintf(stderr, "%d failures\n", failures); return 1; }
    std::puts("plugin core: all checks passed");
    return 0;
}
