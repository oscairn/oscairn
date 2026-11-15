// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's plugin groups: nodes of one worker process whose audio flows between them render there in one round trip,
// each node's input mixed in the worker. Every routing case renders sample for sample and event for event as with
// groups off (OSCAIRN_WORKER_GROUPS=0) in the same host, and as with a worker per plugin: a serial chain, a send tapped
// mid-chain, a split and merge with gains, a live gain moving inside a group, a NaN through the output guard between
// nodes, audio from outside (the input) splitting the group there, notes from the input, a note connection draining (a
// part switched off), a node retiring, and a played change whose gains ramp (that plan forms no groups). Each case
// says which nodes it expects grouped. Then a host killed while a group plays: the group comes back, grouped, and sounds.
// Run under each OSCAIRN_WORKER_REST and each engine core; KICK_FIXTURE_EFFECT_SLEEP makes the effects sleep in silence.
//   oscairn-plugin-groups-test WORKER NORMAL.clap
#include <oscairn/graph.h>
#include <oscairn/realtime.h>
#include <oscairn/plugin_host.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <set>
#include <string>
#include <thread>
#include <vector>

namespace
{
    using kick::clap::Event;
    using oscairn::AudioConnection; using oscairn::NodeId; using oscairn::graphInput; using oscairn::graphOutput;
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }

    constexpr double rate = 48000; constexpr uint32_t frames = 128, blocks = 700;
    const char* worker = nullptr; const char* normal = nullptr;

    void placeIn(uint32_t hosts, std::set<std::string> own = {}) { oscairn::PlacementPolicy p; p.worker = worker; p.hosts = hosts; p.ownHost = std::move(own); oscairn::setPlacementPolicy(p); }

    // Every event each node heard and sent, in order.
    struct Log final : oscairn::Graph::Listener
    {
        std::vector<std::string> lines;
        void events(NodeId node, const Event* e, uint32_t count, bool emitted) noexcept override
        {
            for(uint32_t i = 0; i < count; ++i)
            {
                const auto* h = e[i].header();
                std::string bytes(reinterpret_cast<const char*>(h), h->size);
                lines.push_back(std::to_string(node) + (emitted ? ">" : "<") + std::to_string(h->type) + "@" + std::to_string(h->time) + ":" + bytes);
            }
        }
    };

    struct Player
    {
        oscairn::Graph graph{rate, frames, oscairn::ProcessingMode::Realtime, {}, oscairn::AudioLayout{{{0, "In", 2, 0}}, {{0, "Main", 2, 0}, {1, "Tap", 2, 0}}}};
        std::vector<float> buffer = std::vector<float>(4 * frames), input = std::vector<float>(2 * frames);
        float* out[4]{buffer.data(), buffer.data() + frames, buffer.data() + 2 * frames, buffer.data() + 3 * frames};
        const float* in[2]{input.data(), input.data() + frames};
        Log log;
        Player() { graph.setListener(&log); }
        ~Player() { graph.setListener(nullptr); }
        NodeId add(const char* id) { return graph.add(normal, id); }
        void audio(NodeId from, NodeId to, double gain = 1, uint32_t port = 0, std::array<uint8_t, 4> live = {})
        { graph.connectAudio(AudioConnection{{from, 0, 0}, {to, to == graphOutput ? port : 0, 0}, 2, gain, live}); }
        void play(uint32_t block, const std::vector<Event>& events)
        {
            for(uint32_t i = 0; i < frames; ++i) input[i] = input[frames + i] = block >= 300 && block < 360 ? float(std::sin(.01 * double(block * frames + i))) : 0.f;
            graph.process(in, 2, out, 4, frames, nullptr, events.data(), uint32_t(events.size()));
        }
    };
    // Notes, then a silence long enough for an idle worker to rest (its hold is 192 blocks), the input sounding through it,
    // then notes again.
    std::vector<Event> notes(uint32_t block)
    {
        if(block < 120 || block >= 560)
        {
            if(block % 40 == 0) return {Event::note(true, 60 + int(block / 40 % 12), .8)};
            if(block % 40 == 30) return {Event::note(false, 60 + int(block / 40 % 12), 0)};
        }
        return {};
    }

    struct Take { std::vector<float> audio; std::vector<std::string> events; std::vector<bool> grouped; };
    // A case: builds the graph, returns the nodes whose grouping it checks; at a block, may change the graph (and commit).
    struct Case { std::string name; std::function<std::vector<NodeId>(Player&)> build; std::vector<bool> expected; std::function<void(Player&, uint32_t, const std::vector<NodeId>&)> change; };

    Take take(const Case& c, uint32_t hosts, bool groups)
    {
        placeIn(hosts);
        if(groups) unsetenv("OSCAIRN_WORKER_GROUPS"); else setenv("OSCAIRN_WORKER_GROUPS", "0", 1);
        Take t;
        Player p;
        const auto nodes = c.build(p);
        p.graph.commit();
        for(const auto n : nodes) t.grouped.push_back(p.graph.grouped(n));
        for(uint32_t b = 0; b < blocks; ++b)
        {
            if(c.change) c.change(p, b, nodes);
            p.play(b, notes(b));
            t.audio.insert(t.audio.end(), p.buffer.begin(), p.buffer.end());
        }
        t.events = std::move(p.log.lines);
        unsetenv("OSCAIRN_WORKER_GROUPS");
        return t;
    }

    void compare(const Case& c)
    {
        const auto grouped = take(c, 1, true), apart = take(c, 1, false), own = take(c, 0, true);
        check(grouped.grouped == c.expected, c.name + ": the nodes grouped as the rule says");
        check(std::none_of(apart.grouped.begin(), apart.grouped.end(), [](bool g) { return g; }) && std::none_of(own.grouped.begin(), own.grouped.end(), [](bool g) { return g; }),
              c.name + ": none grouped with groups off or a worker each");
        auto same = [&](const Take& a, const Take& b, const char* what) {
            size_t at = 0; while(at < a.audio.size() && at < b.audio.size() && std::memcmp(&a.audio[at], &b.audio[at], sizeof(float)) == 0) ++at;
            check(a.audio.size() == b.audio.size() && at == a.audio.size(), c.name + ": sample for sample as " + what + (at < a.audio.size() ? " (first differs at block " + std::to_string(at / (4 * frames)) + ")" : ""));
            check(a.events == b.events, c.name + ": event for event as " + what + " (" + std::to_string(a.events.size()) + " against " + std::to_string(b.events.size()) + ")");
        };
        same(grouped, apart, "with groups off");
        same(grouped, own, "with a worker per plugin");
        float loudest = 0; for(const float x : grouped.audio) if(std::isfinite(x)) loudest = std::max(loudest, std::abs(x));
        check(loudest > .01f, c.name + ": it sounds");
        std::printf("%s: %zu samples, %zu events, grouped %d of %zu as expected\n", c.name.c_str(), grouped.audio.size(), grouped.events.size(),
                    int(std::count(grouped.grouped.begin(), grouped.grouped.end(), true)), grouped.grouped.size());
    }

    void setGain(Player& p, NodeId n, double v) { p.graph.host(n).setParameter(7, v); }

    // A publisher on the own-host list (compute.ownHost "org.kick.test.*"): each of its plugins a worker of its own, so no group.
    void trusted()
    {
        placeIn(1, {"org.kick.test.*"});
        Player p;
        const auto inst = p.add("org.kick.test.midi"), a = p.add("org.kick.test.effect"), b = p.add("org.kick.test.effect");
        p.graph.connectNotes(graphInput, inst);
        p.audio(inst, a); p.audio(a, b); p.audio(b, graphOutput);
        p.graph.commit();
        bool pooled = false; for(const auto& h : oscairn::pluginHosts()) pooled = pooled || (h.pooled && !h.plugins.empty());
        check(!p.graph.grouped(b) && !pooled, "own host by publisher: every plugin of org.kick.test.* a worker of its own, none grouped");
    }

    // A host killed while a group plays: the group comes back in its host, grouped, and sounds again.
    void crash()
    {
        placeIn(1);
        unsetenv("OSCAIRN_WORKER_GROUPS");
        Player p;
        const auto inst = p.add("org.kick.test.midi"), a = p.add("org.kick.test.effect"), b = p.add("org.kick.test.effect");
        p.graph.connectNotes(graphInput, inst);
        p.audio(inst, a); p.audio(a, b); p.audio(b, graphOutput);
        p.graph.commit();
        check(!p.graph.grouped(a) && p.graph.grouped(b), "crash: the chain is grouped (the instrument renders through the graph's prepared instances, so the first effect heads it)");
        const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
        auto next = std::chrono::steady_clock::now();
        auto sounding = [&] { float m = 0; for(uint32_t i = 0; i < 2 * frames; ++i) m = std::max(m, std::abs(p.buffer[i])); return m > .01f; };
        bool before = false, silent = false, back = false;
        long killed = 0;
        for(uint32_t block = 0; block < 1500 && !back; ++block)
        {
            std::this_thread::sleep_until(next += period);
            // A note struck again every 100 blocks: a restart replays controllers, not the notes held.
            p.play(1000, block % 100 == 0 ? std::vector<Event>{Event::note(true, 60, .8)} : block % 100 == 99 ? std::vector<Event>{Event::note(false, 60, 0)} : std::vector<Event>{});
            if(block == 200)
            {
                before = sounding();
                for(const auto& h : oscairn::pluginHosts()) if(h.pooled && h.pid > 0 && !h.plugins.empty()) killed = h.pid;
                if(killed) kill(pid_t(killed), SIGKILL);
            }
            if(block > 200 && block % 100 > 5 && block % 100 < 95 && !sounding()) silent = true;
            if(block > 200 && silent && sounding()) back = true;
        }
        check(before && killed, "crash: the group sounded and its host was killed");
        check(back, "crash: the group came back after its host restarted (its held note's controllers replayed, the note itself not)");
        check(p.graph.grouped(b), "crash: still grouped after the restart");
    }
}

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: %s WORKER NORMAL.clap\n", argv[0]); return 2; }
    worker = argv[1]; normal = argv[2];
    const char* inst = "org.kick.test.midi"; const char* fx = "org.kick.test.effect";
    std::vector<Case> cases;
    cases.push_back({"serial chain", [&](Player& p) {
        const auto i = p.add(inst), a = p.add(fx), b = p.add(fx), c = p.add(fx);
        p.graph.connectNotes(graphInput, i);
        p.audio(i, a); p.audio(a, b); p.audio(b, c); p.audio(c, graphOutput);
        setGain(p, b, .8); setGain(p, c, .3);
        return std::vector<NodeId>{i, a, b, c}; }, {false, false, true, true}, {}});
    cases.push_back({"send tapped mid-chain", [&](Player& p) {
        const auto i = p.add(inst), a = p.add(fx), b = p.add(fx), send = p.add(fx);
        p.graph.connectNotes(graphInput, i);
        p.audio(i, a); p.audio(a, b); p.audio(b, graphOutput); p.audio(a, send, .25); p.audio(send, graphOutput, 1, 1); p.audio(a, graphOutput, .5, 1);
        return std::vector<NodeId>{a, b, send}; }, {false, true, true}, {}});
    cases.push_back({"split and merge with gains", [&](Player& p) {
        const auto i = p.add(inst), h = p.add(fx), l = p.add(fx), r = p.add(fx), m = p.add(fx);
        p.graph.connectNotes(graphInput, i);
        p.audio(i, h); p.audio(h, l); p.audio(h, r, .6); p.audio(l, m, .7); p.audio(r, m, .3); p.audio(h, m, .11); p.audio(m, graphOutput);
        setGain(p, r, .2);
        return std::vector<NodeId>{h, l, r, m}; }, {false, true, true, true}, {}});
    cases.push_back({"live gain inside a group", [&](Player& p) {
        const auto i = p.add(inst), a = p.add(fx), b = p.add(fx);
        p.graph.connectNotes(graphInput, i);
        p.audio(i, a); p.audio(a, b, .9, 0, {3, 0, 0, 0}); p.audio(b, graphOutput);
        return std::vector<NodeId>{a, b}; }, {false, true},
        [](Player& p, uint32_t block, const std::vector<NodeId>&) { if(block % 25 == 5) p.graph.setLiveGain(3, float(.2 + .03 * (block % 17)), block % 50 != 5); }});
    cases.push_back({"NaN through the output guard", [&](Player& p) {
        const auto n = p.add("org.kick.test.nan"), a = p.add(fx), b = p.add(fx);
        p.graph.connectNotes(graphInput, n);
        p.audio(n, a); p.audio(a, b); p.audio(b, graphOutput);
        const auto i = p.add(inst); p.graph.connectNotes(graphInput, i); p.audio(i, graphOutput);
        return std::vector<NodeId>{a, b}; }, {false, true}, {}});
    cases.push_back({"audio from outside splits the group", [&](Player& p) {
        const auto i = p.add(inst), a = p.add(fx), b = p.add(fx), c = p.add(fx);
        p.graph.connectNotes(graphInput, i);
        p.audio(i, a); p.audio(a, b); p.audio(graphInput, b, .5); p.audio(b, c); p.audio(c, graphOutput);
        return std::vector<NodeId>{a, b, c}; }, {false, false, true}, {}});
    cases.push_back({"notes from the input into a member", [&](Player& p) {
        const auto i = p.add(inst), a = p.add(fx), b = p.add(fx);
        p.graph.connectNotes(graphInput, i); p.graph.connectNotes(graphInput, b);
        p.audio(i, a); p.audio(a, b); p.audio(b, graphOutput);
        return std::vector<NodeId>{a, b}; }, {false, true}, {}});
    cases.push_back({"a part switched off and on (draining notes)", [&](Player& p) {
        const auto i = p.add(inst), a = p.add(fx), b = p.add(fx);
        p.graph.connectNotes(graphInput, i);
        p.audio(i, a); p.audio(a, b); p.audio(b, graphOutput);
        return std::vector<NodeId>{b, i}; }, {true, false},
        [](Player& p, uint32_t block, const std::vector<NodeId>& n) {
            if(block == 50 || block == 580) { p.graph.setDraining(graphInput, n[1], block == 50); p.graph.commit(); } }});
    cases.push_back({"a node retiring", [&](Player& p) {
        const auto i = p.add(inst), a = p.add(fx), b = p.add(fx);
        p.graph.connectNotes(graphInput, i);
        p.audio(i, a); p.audio(a, b); p.audio(b, graphOutput);
        return std::vector<NodeId>{a, b}; }, {false, true},
        [](Player& p, uint32_t block, const std::vector<NodeId>& n) { if(block == 90) { p.graph.setRetiring(n[1], true); p.graph.commit(); } }});
    cases.push_back({"a played change ramping a gain inside a group", [&](Player& p) {
        const auto i = p.add(inst), a = p.add(fx), b = p.add(fx);
        p.graph.connectNotes(graphInput, i);
        p.audio(i, a); p.audio(a, b, .9); p.audio(b, graphOutput);
        return std::vector<NodeId>{a, b}; }, {false, true},
        [](Player& p, uint32_t block, const std::vector<NodeId>& n) {
            if(block == 60 || block == 600) { p.graph.disconnectAudio(n[0], n[1]); p.audio(n[0], n[1], block == 60 ? .4 : .7); p.graph.commit(true); }
            const bool groups = oscairn::worker::workerGroups() && oscairn::placementPolicy().hosts;
            if(block == 600) check(!p.graph.grouped(n[1]), "ramped: a played change's plan forms no groups");
            if(block == 640) { p.graph.commit(); check(p.graph.grouped(n[1]) == groups, "ramped: the next plan groups again"); } }});
    for(const auto& c : cases) compare(c);
    trusted();
    crash();
    if(failures) std::fprintf(stderr, "%d failure(s)\n", failures);
    else std::printf("plugin groups: every case as with groups off\n");
    return failures ? 1 : 0;
}
