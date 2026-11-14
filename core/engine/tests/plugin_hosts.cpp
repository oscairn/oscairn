// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's plugin hosts: plugins of any kind share a fixed pool of worker processes, placed by
// measured cost; every node renders exactly as in the engine; killing a host while it plays leaves
// the engine and the other host playing without a missed block, and brings the dead host's instances
// back; a plugin that crashes a shared host gets a worker of its own and the others come back in the
// host; loading and unloading instances in a running host leaves the instance playing there alone; a
// plugin too slow for its host is put down to and gets a worker of its own; a host failing again and
// again for no plugin known (memory) splits into workers of their own rather than silencing them all.
//   oscairn-plugin-hosts-test WORKER NORMAL.clap NOTE_CRASH.clap SLOW.clap LEAK.clap [--no-timing]
// --no-timing reports the missed blocks and callbacks past the period instead of failing on them.
#include <oscairn/graph.h>
#include <oscairn/realtime.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <thread>
#include <vector>

namespace
{
    using kick::clap::Event;
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }

    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
    const char* worker = nullptr; const char* normal = nullptr; const char* noteCrash = nullptr; const char* slow = nullptr; const char* leak = nullptr;
    bool timing = true;

    // memory: each host's limit; one not yet used also gives hosts of its own, apart from the earlier cases'.
    void placeIn(uint32_t hosts, uint64_t memory = uint64_t(4) << 30) { oscairn::PlacementPolicy p; p.worker = hosts ? worker : ""; p.hosts = hosts; p.memoryLimit = memory; oscairn::setPlacementPolicy(p); }
    std::vector<oscairn::PluginHostInfo> pooled()
    {
        auto all = oscairn::pluginHosts();
        std::erase_if(all, [](const auto& h) { return !h.pooled; });
        return all;
    }
    size_t count(const std::vector<std::string>& plugins, const std::string& id) { return size_t(std::count(plugins.begin(), plugins.end(), id)); }

    // Two stereo outputs: port 0 for the nodes under test, port 1 for the one beside them.
    struct Player
    {
        oscairn::Graph graph{rate, frames, oscairn::ProcessingMode::Realtime, {}, oscairn::AudioLayout{{}, {{0, "Main", 2, 0}, {1, "Other", 2, 0}}}};
        std::vector<float> buffer = std::vector<float>(4 * frames);
        float* out[4]{buffer.data(), buffer.data() + frames, buffer.data() + 2 * frames, buffer.data() + 3 * frames};
        oscairn::NodeId add(const char* path, const char* id, uint32_t port, uint32_t channels = 2)
        {
            const auto n = graph.add(path, id);
            graph.connectNotes(oscairn::graphInput, n);
            graph.connectAudio(oscairn::AudioConnection{{n, 0, 0}, {oscairn::graphOutput, port, 0}, channels});
            return n;
        }
        // One block, as a device's callback runs it: its deadline set (the engine's reserve kept); false past the period.
        // Without a callback's deadline, as worker-failures plays: a worker's block past its allowance is missed.
        void playUnbounded(const std::vector<Event>& events) { graph.process(nullptr, 0, out, 4, frames, nullptr, events.data(), uint32_t(events.size())); }
        bool play(const std::vector<Event>& events = {})
        {
            const auto start = std::chrono::steady_clock::now();
            oscairn::worker::callbackDeadline = start + std::chrono::nanoseconds(int64_t(double(period.count()) * (1 - oscairn::worker::mixingReserve)));
            graph.process(nullptr, 0, out, 4, frames, nullptr, events.data(), uint32_t(events.size()));
            oscairn::worker::callbackDeadline = {};
            return std::chrono::steady_clock::now() - start <= period;
        }
        std::vector<float> port(uint32_t p) const { return {buffer.begin() + 2 * p * frames, buffer.begin() + 2 * (p + 1) * frames}; }
    };
    // A note on every 40 blocks, off 30 blocks later.
    std::vector<Event> notes(uint32_t block)
    {
        if(block % 40 == 0) return {Event::note(true, 60 + int(block / 40 % 12), .8)};
        if(block % 40 == 30) return {Event::note(false, 60 + int(block / 40 % 12), 0)};
        return {};
    }

    // Placement by measured cost, any plugins in one host; a host stays when it is empty.
    void placement()
    {
        placeIn(2);
        oscairn::setPluginCost("org.kick.test.effect", .5);
        std::vector<std::shared_ptr<oscairn::PluginHost>> loaded;
        loaded.push_back(oscairn::loadPlugin(normal, "org.kick.test.effect", rate, frames));
        for(int i = 0; i < 6; ++i) loaded.push_back(oscairn::loadPlugin(normal, "org.kick.test.midi", rate, frames));
        auto hosts = pooled();
        check(hosts.size() == 2, "two plugin hosts for seven instances: " + std::to_string(hosts.size()));
        if(hosts.size() == 2)
        {
            const auto& heavy = count(hosts[0].plugins, "org.kick.test.effect") ? hosts[0] : hosts[1];
            const auto& light = &heavy == &hosts[0] ? hosts[1] : hosts[0];
            // The effect at .5 alone until five instruments at .1 make the other host as costly; the sixth joins the effect.
            check(heavy.plugins.size() == 2 && count(heavy.plugins, "org.kick.test.midi") == 1 && count(light.plugins, "org.kick.test.midi") == 5,
                  "placed by cost: " + std::to_string(heavy.plugins.size()) + " beside the effect, " + std::to_string(light.plugins.size()) + " in the other host");
            check(std::abs(heavy.cost - .6) < 1e-9 && std::abs(light.cost - .5) < 1e-9, "each host's cost: " + std::to_string(heavy.cost) + ", " + std::to_string(light.cost));
        }
        const auto before = pooled();
        loaded.clear();
        const auto after = pooled();
        bool same = after.size() == before.size();
        for(size_t i = 0; same && i < after.size(); ++i) same = after[i].pid == before[i].pid && after[i].pid > 0 && after[i].plugins.empty();
        check(same, "the hosts stay, empty, when their instances go");
        oscairn::setPluginCost("org.kick.test.effect", 0);
    }

    // Instruments and an effect after one of them, in two hosts, sound exactly as in the engine.
    void identical()
    {
        auto build = [](Player& p, uint32_t hosts) {
            placeIn(hosts);
            const auto a = p.add(normal, "org.kick.test.midi", 0); p.add(normal, "org.kick.test.midi", 0); p.add(normal, "org.kick.test.midi", 1);
            const auto fx = p.graph.add(normal, "org.kick.test.effect");
            p.graph.host(a).setParameter(7, .3);
            p.graph.connectAudio(oscairn::AudioConnection{{a, 0, 0}, {fx, 0, 0}, 2});
            p.graph.connectAudio(oscairn::AudioConnection{{fx, 0, 0}, {oscairn::graphOutput, 1, 0}, 2});
            p.graph.commit();
        };
        Player hosted, engine;
        build(hosted, 2); build(engine, 0);
        bool same = true; double energy = 0;
        for(uint32_t block = 0; block < 400; ++block)
        {
            hosted.play(notes(block)); engine.play(notes(block));
            same = same && hosted.buffer == engine.buffer;
            for(float x : engine.buffer) energy += double(x) * x;
        }
        check(energy > 0 && same, "three instruments and an effect in plugin hosts sound as in the engine");
    }

    // The owner's question: a host killed while it plays. The other host plays on, every block on time
    // and exactly as before; the engine's callbacks stay within the period; the dead host's two
    // instances come back and play again. Effects on the graph's input: plugins handed their blocks over
    // and collected, as the Sampler and Analog are (the fixture's instrument renders through the
    // graph's prepared instances, which wait for each block in turn).
    void killed()
    {
        const auto layout = oscairn::AudioLayout{{{0, "In", 2, 0}}, {{0, "Main", 2, 0}, {1, "Other", 2, 0}}};
        oscairn::Graph hosted(rate, frames, oscairn::ProcessingMode::Realtime, {}, layout), reference(rate, frames, oscairn::ProcessingMode::Realtime, {}, layout);
        auto add = [](oscairn::Graph& g, uint32_t port, double gain) {
            const auto n = g.add(normal, "org.kick.test.effect");
            g.host(n).setParameter(7, gain);
            g.connectAudio(oscairn::AudioConnection{{oscairn::graphInput, 0, 0}, {n, 0, 0}, 2});
            g.connectAudio(oscairn::AudioConnection{{n, 0, 0}, {oscairn::graphOutput, port, 0}, 2});
            return n;
        };
        placeIn(2);
        // Placed by cost, the hosts empty: the first and third in one host, the second in the other.
        const auto a1 = add(hosted, 0, .25), b = add(hosted, 1, .5), a2 = add(hosted, 0, .125);
        hosted.commit();
        placeIn(0);
        add(reference, 0, .25); add(reference, 1, .5); add(reference, 0, .125);
        reference.commit();
        long victim = 0;
        for(const auto& h : pooled()) if(count(h.plugins, "org.kick.test.effect") == 2) victim = h.pid;
        check(victim > 0, "two effects share a host");
        std::vector<float> input(2 * frames, .5f), mine(4 * frames), theirs(4 * frames);
        const float* in[2]{input.data(), input.data() + frames};
        float* out[4]{mine.data(), mine.data() + frames, mine.data() + 2 * frames, mine.data() + 3 * frames};
        float* ref[4]{theirs.data(), theirs.data() + frames, theirs.data() + 2 * frames, theirs.data() + 3 * frames};
        auto play = [&](oscairn::Graph& g, float* const* o) {
            const auto start = std::chrono::steady_clock::now();
            oscairn::worker::callbackDeadline = start + std::chrono::nanoseconds(int64_t(double(period.count()) * (1 - oscairn::worker::mixingReserve)));
            g.process(in, 2, o, 4, frames, nullptr);
            oscairn::worker::callbackDeadline = {};
            return std::chrono::steady_clock::now() - start <= period;
        };
        auto port = [](const std::vector<float>& v, uint32_t p) { return std::vector<float>(v.begin() + 2 * p * frames, v.begin() + 2 * (p + 1) * frames); };
        oscairn::worker::makeRealtime(1e6 * frames / rate, oscairn::worker::priority::engineAudio);
        uint32_t over = 0, block = 0, back = 0; bool steady = true; constexpr uint32_t kill = 200;
        auto next = std::chrono::steady_clock::now();
        for(; block < 1200; ++block)
        {
            std::this_thread::sleep_until(next); next += period;
            // From another thread, as a crash comes: not a system call of the audio thread's.
            if(block == kill && victim > 0) std::thread([victim] { ::kill(pid_t(victim), SIGKILL); }).detach();
            if(!play(hosted, out)) ++over;
            play(reference, ref);
            if(steady && port(mine, 1) != port(theirs, 1)) std::fprintf(stderr, "the other host's effect differs at block %u (the kill at %u)\n", block, kill);
            steady = steady && port(mine, 1) == port(theirs, 1);
            if(block > kill && !back && hosted.host(a1).statistics().restarts && hosted.host(a2).statistics().restarts) back = block;
        }
        const auto other = hosted.host(b).statistics();
        // Without real time (sanitizers, virtual machines) the other host may miss blocks of its own: its sound
        // must then match only where it missed none, as in worker-failures.
        if(timing || other.lateBlocks + other.silentBlocks == 0) check(steady, "the other host's effect sounds exactly as without the kill, every block");
        check(back, "the killed host's two effects came back (block " + std::to_string(back) + ")");
        const auto restored = hosted.host(a1).statistics(), again = hosted.host(a2).statistics();
        if(timing || restored.lateBlocks + restored.silentBlocks + again.lateBlocks + again.silentBlocks == 0)
            check(port(mine, 0) == port(theirs, 0), "and play as before");
        std::printf("killed host: %u blocks, %u callbacks past the period, the other host's late %llu silent %llu, back at block %u\n", block, over,
                    (unsigned long long)other.lateBlocks, (unsigned long long)other.silentBlocks, back);
        if(timing)
        {
            check(other.lateBlocks == 0 && other.silentBlocks == 0, "the other host missed no block: late " + std::to_string(other.lateBlocks) + ", silent " + std::to_string(other.silentBlocks));
            check(over == 0, "no callback past the period: " + std::to_string(over));
        }
    }

    // A plugin crashing a shared host goes to a worker of its own and stays there; the other comes back in the host.
    void separated()
    {
        placeIn(2);
        oscairn::setPluginCost("org.kick.test.effect", .5);
        auto filler = oscairn::loadPlugin(normal, "org.kick.test.effect", rate, frames); // the costliest host: both below go to the other
        Player p;
        const auto a = p.add(normal, "org.kick.test.midi", 0), c = p.add(noteCrash, "org.kick.test.synth", 1, 1); // a mono instrument
        p.graph.commit();
        long shared = 0;
        for(const auto& h : pooled()) if(count(h.plugins, "org.kick.test.midi") && count(h.plugins, "org.kick.test.synth")) shared = h.pid;
        check(shared > 0, "the instrument and the crashing one share a host");
        auto next = std::chrono::steady_clock::now();
        uint32_t block = 0;
        for(; block < 1500; ++block)
        {
            std::this_thread::sleep_until(next); next += period;
            p.play(block == 20 ? std::vector<Event>{Event::note(true, 0, 1)} : std::vector<Event>{}); // key 0 crashes the note_crash fixture
            if(block > 20 && p.graph.host(a).statistics().restarts && p.graph.host(c).statistics().restarts && p.graph.host(c).failure().empty()) break;
        }
        check(p.graph.host(a).statistics().restarts == 1 && p.graph.host(c).statistics().restarts == 1, "both came back, once each");
        bool own = false, back = false;
        for(const auto& h : oscairn::pluginHosts())
        {
            if(!h.pooled && h.plugins == std::vector<std::string>{"org.kick.test.synth"} && h.pid > 0) own = true;
            if(h.pooled && count(h.plugins, "org.kick.test.midi") && !count(h.plugins, "org.kick.test.synth")) back = true;
        }
        check(own, "the crashing plugin is in a worker of its own");
        check(back, "the other is back in a plugin host, without it");
        auto later = oscairn::loadPlugin(normal, "org.kick.test.synth", rate, frames);
        size_t alone = 0;
        for(const auto& h : oscairn::pluginHosts()) if(!h.pooled && h.plugins == std::vector<std::string>{"org.kick.test.synth"}) ++alone;
        check(alone == 2, "a later instance of that plugin gets a worker of its own too: " + std::to_string(alone));
        oscairn::setPluginCost("org.kick.test.effect", 0);
    }

    // A plugin too slow in a shared host (the slow fixture, 2 ms a block) is put down to: the host's blocks are all
    // late behind it, but it alone goes to a worker of its own (and gives up there), and the other comes back in
    // the host once and plays on. Its ID gets a worker of its own from then on: last of the cases using it.
    void tooSlow()
    {
        placeIn(1, uint64_t(3) << 30); // a host apart from the earlier cases'
        Player p;
        const auto a = p.add(normal, "org.kick.test.midi", 0), s = p.add(slow, "org.kick.test.effect", 1);
        p.graph.commit();
        auto next = std::chrono::steady_clock::now();
        for(uint32_t block = 0; block < 3000; ++block)
        {
            std::this_thread::sleep_until(next); next += period;
            p.playUnbounded(notes(block));
            if(p.graph.host(s).failure().find("not restarted") != std::string::npos && p.graph.host(a).failure().empty() && block % 40 == 39) break;
        }
        const auto failure = p.graph.host(a).failure();
        { const auto st = p.graph.host(s).statistics(); std::printf("too slow: slow plugin %llu blocks, last %.0f us, late %llu, silent %llu, misses %llu, restarts %llu\n", (unsigned long long)st.blocks, st.lastMicroseconds, (unsigned long long)st.lateBlocks, (unsigned long long)st.silentBlocks, (unsigned long long)st.deadlineMisses, (unsigned long long)st.restarts); }
        check(failure.empty() && p.graph.host(a).statistics().restarts == 1, "the instrument beside a plugin too slow comes back once and plays on: " + std::to_string(p.graph.host(a).statistics().restarts) + " restarts, \"" + failure + "\"");
        check(p.graph.host(s).failure().find("too slow") != std::string::npos, "the slow plugin is the one stopped: \"" + p.graph.host(s).failure() + "\"");
        bool own = false;
        for(const auto& h : oscairn::pluginHosts()) if(!h.pooled && h.plugins == std::vector<std::string>{"org.kick.test.effect"}) own = true;
        check(own, "the slow plugin is in a worker of its own");
    }

    // A host stopped again and again for memory, which no plugin is put down to (the leak fixture, 1 MiB a block
    // past a 128 MiB limit), is not given up with every plugin in it silent: its plugins go to workers of their own,
    // and only the leaking one is given up.
    void leaking()
    {
        placeIn(1, uint64_t(128) << 20);
        Player p;
        const auto a = p.add(normal, "org.kick.test.midi", 0), l = p.add(leak, "org.kick.test.stepped", 1); // an ID no earlier case put in a worker of its own
        p.graph.commit();
        auto next = std::chrono::steady_clock::now();
        for(uint32_t block = 0; block < 6000; ++block)
        {
            std::this_thread::sleep_until(next); next += period;
            p.playUnbounded(notes(block));
            if(p.graph.host(l).failure().find("not restarted") != std::string::npos && p.graph.host(a).failure().empty() && block % 40 == 39) break;
        }
        check(p.graph.host(l).failure().find("too much memory") != std::string::npos && p.graph.host(l).failure().find("not restarted") != std::string::npos,
              "the leaking plugin is given up: \"" + p.graph.host(l).failure() + "\"");
        check(p.graph.host(a).failure().empty(), "the instrument beside it is not: \"" + p.graph.host(a).failure() + "\"");
        bool apart = true;
        for(const auto& h : oscairn::pluginHosts()) if(count(h.plugins, "org.kick.test.midi") && count(h.plugins, "org.kick.test.stepped")) apart = false;
        check(apart, "the two are in workers of their own");
    }

    // Stage 4's hook: instances loaded into and unloaded from the running hosts while one plays.
    void residency()
    {
        Player hosted, reference;
        placeIn(1);
        const auto a = hosted.add(normal, "org.kick.test.midi", 0);
        hosted.graph.commit();
        placeIn(0);
        reference.add(normal, "org.kick.test.midi", 0); reference.graph.commit();
        placeIn(1);
        std::atomic<bool> done{false};
        bool same = true; uint32_t over = 0;
        std::thread audio([&] {
            oscairn::worker::makeRealtime(1e6 * frames / rate, oscairn::worker::priority::engineAudio);
            auto next = std::chrono::steady_clock::now();
            for(uint32_t block = 0; !done.load() || block % 40; ++block)
            {
                std::this_thread::sleep_until(next); next += period;
                if(!hosted.play(notes(block))) ++over;
                reference.play(notes(block));
                same = same && hosted.buffer == reference.buffer;
            }
        });
        for(int round = 0; round < 5; ++round)
        {
            std::vector<std::shared_ptr<oscairn::PluginHost>> more;
            for(int i = 0; i < 3; ++i) more.push_back(oscairn::loadPlugin(normal, i == 1 ? "org.kick.test.effect" : "org.kick.test.midi", rate, frames));
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
        }
        done = true; audio.join();
        const auto st = hosted.graph.host(a).statistics();
        if(timing || st.lateBlocks + st.silentBlocks == 0) check(same, "an instrument sounds as before while instances load into its host and leave it");
        std::printf("residency: %u callbacks past the period, late %llu, silent %llu\n", over, (unsigned long long)st.lateBlocks, (unsigned long long)st.silentBlocks);
        if(timing) check(st.lateBlocks == 0 && st.silentBlocks == 0 && over == 0, "and on time");
    }
}

int main(int argc, char** argv)
{
    if(argc < 6) { std::fprintf(stderr, "usage: %s WORKER NORMAL.clap NOTE_CRASH.clap SLOW.clap LEAK.clap [--no-timing]\n", argv[0]); return 2; }
    worker = argv[1]; normal = argv[2]; noteCrash = argv[3]; slow = argv[4]; leak = argv[5];
    timing = !(argc > 6 && std::strcmp(argv[6], "--no-timing") == 0);
    try
    {
        placement();
        identical();
        killed();
        separated();
        residency();
        tooSlow();
        leaking();
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    return failures ? 1 : 0;
}
