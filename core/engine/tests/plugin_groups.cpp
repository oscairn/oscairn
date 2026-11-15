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
// With the Basic Synth: under each placement policy (plugin, chain, pool 3), Sick tines' shape (a shared instance over
// five buses, five four-deep chains into one master), two instruments sharing an effect bus and the 22-plugin shape
// render sample for sample as with a worker each; which nodes share a process and a round trip; and with chain, one
// part's process killed mid-play: only that part goes silent, and it comes back.
//   oscairn-plugin-groups-test WORKER NORMAL.clap [BASIC_SYNTH.clap]
#include <oscairn/graph.h>
#include <oscairn/realtime.h>
#include <oscairn/plugin_host.h>
#include <oscairn/sound_context.h>
#include <map>
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
    const char* worker = nullptr; const char* normal = nullptr; const char* synth = nullptr;

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

namespace
{
    // The placement policies (compute.workers): plugin, chain, pool 3. Shapes built as a Session places a Performance's
    // plugins: each loaded under the group placementGroups gives it from the shape's audio wires.
    // pool 1: every plugin of the Performance in one process, the smallest devices' choice.
    enum class Policy { plugin, chain, pool, pool1 };
    const char* name(Policy p) { return p == Policy::plugin ? "plugin" : p == Policy::chain ? "chain" : p == Policy::pool ? "pool 3" : "pool 1"; }
    void place(Policy p, std::set<std::string> own = {})
    { oscairn::PlacementPolicy q; q.worker = worker; q.hosts = p == Policy::pool ? 3 : p == Policy::pool1 ? 1 : 0; q.chain = p == Policy::chain; q.ownHost = std::move(own); oscairn::setPlacementPolicy(q); }

    constexpr uint32_t ports = 6; // the master, then a tap of each part's chain
    std::vector<kick::clap::Port> wideLayout() { std::vector<kick::clap::Port> o; for(uint32_t i = 0; i < ports; ++i) o.push_back({i, "Out " + std::to_string(i), 2, 0}); return o; }
    struct Wide
    {
        oscairn::Graph graph{rate, frames, oscairn::ProcessingMode::Realtime, {}, oscairn::AudioLayout{{{0, "In", 2, 0}}, wideLayout()}};
        std::vector<float> buffer = std::vector<float>(2 * ports * frames), input = std::vector<float>(2 * frames);
        std::vector<float*> out; const float* in[2]{input.data(), input.data() + frames};
        std::map<std::string, oscairn::PlacementGroup> groups;
        std::map<std::string, NodeId> nodes;
        Wide() { for(uint32_t c = 0; c < 2 * ports; ++c) out.push_back(buffer.data() + c * frames); }
        // Groups from the shape's wires, as performance.cpp's addNodes takes them from a document.
        void plan(const std::vector<std::string>& names, const std::vector<std::pair<std::string, std::string>>& audio)
        { static int loads = 0; groups = oscairn::placementGroups(names, audio, "test-" + std::to_string(++loads) + "/"); }
        NodeId add(const std::string& n, const char* id) { const oscairn::PlacementScope s(groups[n]); return nodes[n] = graph.add(normal, id); }
        void wire(const std::string& from, const std::string& to, double l = 1, double r = 1)
        {
            graph.connectAudio(AudioConnection{{nodes.at(from), 0, 0}, {nodes.at(to), 0, 0}, 1, l});
            graph.connectAudio(AudioConnection{{nodes.at(from), 0, 1}, {nodes.at(to), 0, 1}, 1, r});
        }
        void toPort(const std::string& from, uint32_t port) { graph.connectAudio(AudioConnection{{nodes.at(from), 0, 0}, {graphOutput, port, 0}, 2}); }
        void play(uint32_t block, const std::vector<Event>& events)
        {
            for(uint32_t i = 0; i < frames; ++i) input[i] = input[frames + i] = float(.2 * std::sin(.013 * double(block * frames + i)));
            // paced: as a device's callback runs it, its deadline set (the engine's reserve kept), so a dead worker costs a block.
            if(paced) oscairn::worker::callbackDeadline = std::chrono::steady_clock::now() + std::chrono::nanoseconds(int64_t(1e9 * frames / rate * (1 - oscairn::worker::mixingReserve)));
            graph.process(in, 2, out.data(), 2 * ports, frames, nullptr, events.data(), uint32_t(events.size()));
            oscairn::worker::callbackDeadline = {};
        }
        bool paced = false;
        float peak(uint32_t port) const { float m = 0; for(uint32_t i = 0; i < 2 * frames; ++i) m = std::max(m, std::abs(buffer[2 * port * frames + i])); return m; }
    };

    // Sick tines on the FANTOM-07: one shared Basic Synth playing five parts (sound contexts on buses 0 to 4), each part a
    // serial chain of four effects (chorus, rotary, 3-band EQ, 2-band EQ), all five into one Master EQ with the part's
    // level and pan and two sends tapped after the last insert, summed per channel as a Session compiles the three wires.
    struct Tines
    {
        std::vector<std::string> chain(int p) { std::vector<std::string> c; for(const char* e : {"chorus", "rotary", "eq3", "eq2"}) c.push_back("p" + std::to_string(p) + e); return c; }
        void build(Wide& w)
        {
            std::vector<std::string> names{"fm"}; std::vector<std::pair<std::string, std::string>> audio;
            for(int p = 0; p < 5; ++p)
            {
                names.push_back("fm" + std::to_string(p)); auto c = chain(p); names.insert(names.end(), c.begin(), c.end());
                audio.emplace_back("fm" + std::to_string(p), c[0]);
                for(size_t k = 0; k < 3; ++k) audio.emplace_back(c[k], c[k + 1]);
                audio.emplace_back(c[3], "master");
            }
            names.push_back("master");
            w.plan(names, audio);
            std::shared_ptr<oscairn::PluginHost> plugin;
            { const oscairn::PlacementScope s(w.groups["fm0"]); plugin = oscairn::loadPlugin(synth, "", rate, frames); }
            auto engine = std::make_shared<oscairn::SoundContext::Engine>(); engine->host = plugin; engine->busParameter = oscairn::SoundContext::busParameter(*plugin);
            plugin->setActiveOutputs(0b11111);
            for(int p = 0; p < 5; ++p)
            {
                // Each part its own sound (its Output bus is the sound's), as a Session prepares them.
                const auto sound = plugin->prepareSound("Part " + std::to_string(p), {{1, .3 + .1 * p}, {4, 1}, {5, double(3 + p)}});
                const auto n = w.nodes["fm" + std::to_string(p)] = w.graph.add(std::make_shared<oscairn::SoundContext>(engine, uint16_t(p), uint32_t(p), sound));
                oscairn::NoteZone zone; zone.lowKey = uint8_t(p * 24); zone.highKey = uint8_t(p * 24 + 23);
                w.graph.connectNotes(graphInput, n, {zone});
                for(const auto& e : chain(p)) w.add(e, "org.kick.test.effect");
            }
            w.add("master", "org.kick.test.effect");
            for(int p = 0; p < 5; ++p)
            {
                const auto c = chain(p);
                w.wire("fm" + std::to_string(p), c[0]);
                for(size_t k = 0; k < 3; ++k) w.wire(c[k], c[k + 1]);
                const double level = p == 0 ? .62 : .88, send = ((p == 4 ? .081 : .462) + .323) * level; // the sends at send x part level
                const double left = p == 0 ? level : p % 2 ? level : 0, right = p == 0 ? level : p % 2 ? 0 : level; // pan 0, -1, +1
                w.wire(c[3], "master", left + send, right + send);
                w.toPort(c[3], uint32_t(p + 1));
            }
            w.toPort("master", 0);
        }
    };
    std::vector<Event> keys(uint32_t block)
    {
        std::vector<Event> e;
        if(block % 60 == 0) for(int p = 0; p < 5; ++p) e.push_back(Event::note(true, p * 24 + 12, .8));
        if(block % 60 == 45) for(int p = 0; p < 5; ++p) e.push_back(Event::note(false, p * 24 + 12, 0));
        return e;
    }
    // Two instruments, each through two inserts, sending to one effect bus of two effects.
    void twoOnABus(Wide& w)
    {
        w.plan({"i1", "a1", "b1", "i2", "a2", "b2", "bus1", "bus2"}, {{"i1", "a1"}, {"a1", "b1"}, {"i2", "a2"}, {"a2", "b2"}, {"b1", "bus1"}, {"b2", "bus1"}, {"bus1", "bus2"}});
        for(const char* n : {"i1", "i2"}) { w.add(n, "org.kick.test.midi"); w.graph.connectNotes(graphInput, w.nodes[n]); }
        for(const char* n : {"a1", "b1", "a2", "b2", "bus1", "bus2"}) w.add(n, "org.kick.test.effect");
        w.wire("i1", "a1"); w.wire("a1", "b1"); w.wire("i2", "a2", .7, .7); w.wire("a2", "b2"); w.wire("b1", "bus1", .3, .3); w.wire("b2", "bus1", .5, .5); w.wire("bus1", "bus2");
        w.toPort("b1", 1); w.toPort("b2", 2); w.toPort("bus2", 0);
    }
    // The bench's 22: an effect in the instrument's place fed by the input, five parts of three inserts, five sends every
    // part feeds, a master.
    void twentyTwo(Wide& w)
    {
        std::vector<std::string> names{"inst"}; std::vector<std::pair<std::string, std::string>> audio;
        for(int p = 0; p < 5; ++p) for(int k = 0; k < 3; ++k) { const auto n = "p" + std::to_string(p) + "-" + std::to_string(k); names.push_back(n); audio.emplace_back(k ? "p" + std::to_string(p) + "-" + std::to_string(k - 1) : "inst", n); }
        for(int s = 0; s < 5; ++s) { names.push_back("s" + std::to_string(s)); for(int p = 0; p < 5; ++p) audio.emplace_back("p" + std::to_string(p) + "-2", "s" + std::to_string(s)); }
        names.push_back("master");
        for(int p = 0; p < 5; ++p) audio.emplace_back("p" + std::to_string(p) + "-2", "master");
        for(int s = 0; s < 5; ++s) audio.emplace_back("s" + std::to_string(s), "master");
        w.plan(names, audio);
        for(const auto& n : names) w.add(n, "org.kick.test.effect");
        w.graph.connectAudio(AudioConnection{{graphInput, 0, 0}, {w.nodes["inst"], 0, 0}, 2});
        for(const auto& [a, b] : audio) { const bool send = b[0] == 's' && a[0] == 'p'; w.wire(a, b, send ? .2 : 1, send ? .2 : .9); }
        w.toPort("master", 0);
    }

    struct Placed { std::vector<float> audio; std::map<std::string, const void*> lanes; std::map<std::string, bool> grouped; size_t processes = 0; };
    Placed run(Policy policy, const std::function<void(Wide&)>& build, const std::function<std::vector<Event>(uint32_t)>& events, std::set<std::string> own = {})
    {
        place(policy, std::move(own));
        Placed r;
        Wide w; build(w); w.graph.commit();
        for(const auto& [n, id] : w.nodes) { r.lanes[n] = w.graph.host(id).lane(); r.grouped[n] = w.graph.grouped(id); }
        std::set<const void*> distinct; for(const auto& [n, l] : r.lanes) distinct.insert(l);
        r.processes = distinct.size();
        for(uint32_t b = 0; b < 500; ++b) { w.play(b, events(b)); r.audio.insert(r.audio.end(), w.buffer.begin(), w.buffer.end()); }
        return r;
    }
    bool together(const Placed& r, const std::vector<std::string>& a) { return std::all_of(a.begin(), a.end(), [&](const auto& n) { return r.lanes.at(n) == r.lanes.at(a.front()); }); }
    bool apart(const Placed& r, const std::string& a, const std::string& b) { return r.lanes.at(a) != r.lanes.at(b); }

    void policies()
    {
        Tines tines;
        struct Shape { std::string name; std::function<void(Wide&)> build; std::function<std::vector<Event>(uint32_t)> events; size_t shared; };
        const std::vector<Shape> shapes{
            {"Sick tines", [&](Wide& w) { tines.build(w); }, keys, 4}, // its five sound contexts one instance
            {"two instruments sharing an effect bus", twoOnABus, notes, 0},
            {"the 22-plugin shape", twentyTwo, [](uint32_t) { return std::vector<Event>{}; }, 0}};
        for(const auto& shape : shapes)
        {
            const auto reference = run(Policy::plugin, shape.build, shape.events);
            check(reference.processes == reference.lanes.size() - shape.shared, shape.name + ", plugin: a process each (a shared instance one): " + std::to_string(reference.processes));
            check(std::none_of(reference.grouped.begin(), reference.grouped.end(), [](const auto& g) { return g.second; }), shape.name + ", plugin: no group");
            for(const auto policy : {Policy::chain, Policy::pool, Policy::pool1})
            {
                const auto r = run(policy, shape.build, shape.events);
                size_t at = 0; while(at < r.audio.size() && at < reference.audio.size() && std::memcmp(&r.audio[at], &reference.audio[at], sizeof(float)) == 0) ++at;
                check(at == reference.audio.size() && r.audio.size() == reference.audio.size(), shape.name + ", " + name(policy) + ": sample for sample as plugin" + (at < r.audio.size() ? " (first differs at block " + std::to_string(at / (2 * ports * frames)) + ")" : ""));
                float loudest = 0; for(const float x : r.audio) loudest = std::max(loudest, std::abs(x));
                check(loudest > .001f, shape.name + ", " + name(policy) + ": it sounds");
                std::printf("%s, %s: %zu processes, %d of %zu nodes in their sources' round trip\n", shape.name.c_str(), name(policy), r.processes,
                            int(std::count_if(r.grouped.begin(), r.grouped.end(), [](const auto& g) { return g.second; })), r.grouped.size());
            }
        }
        // What shares a process and a round trip.
        auto tinesUnder = [&](Policy p) { return run(p, [&](Wide& w) { tines.build(w); }, keys); };
        const auto chain = tinesUnder(Policy::chain), pool = tinesUnder(Policy::pool);
        bool chains = true, round = true;
        for(int p = 0; p < 5; ++p)
        {
            const auto c = tines.chain(p);
            chains = chains && together(chain, c) && apart(chain, c[0], "fm0") && apart(chain, c[0], "master") && (p == 0 || apart(chain, c[0], tines.chain(p - 1)[0]));
            round = round && !chain.grouped.at(c[0]) && chain.grouped.at(c[1]) && chain.grouped.at(c[2]) && chain.grouped.at(c[3]);
            check(together(pool, c) && apart(pool, c[0], "fm0"), "Sick tines, pool 3: part " + std::to_string(p) + "'s chain in one host, apart from the instrument's");
        }
        check(chains && chain.processes == 7, "Sick tines, chain: the instrument alone, each chain a process of its own, the master its own: 7 processes, " + std::to_string(chain.processes));
        check(round && !chain.grouped.at("master"), "Sick tines, chain: each chain one round trip from its first insert; the master its own (five groups feed it)");
        check(pool.processes == 3, "Sick tines, pool 3: three hosts, " + std::to_string(pool.processes));
        // pool 1: one process for everything, the whole graph one round trip after the shared instance's.
        const auto one = tinesUnder(Policy::pool1);
        check(one.processes == 1 && std::count_if(one.grouped.begin(), one.grouped.end(), [](const auto& g) { return g.second; }) == 21,
              "Sick tines, pool 1: one process; every effect in the shared instance's round trip: " + std::to_string(one.processes));
        const auto all = run(Policy::pool1, twentyTwo, [](uint32_t) { return std::vector<Event>{}; });
        check(all.processes == 1 && std::count_if(all.grouped.begin(), all.grouped.end(), [](const auto& g) { return g.second; }) == 21, "22-plugin shape, pool 1: one process, one round trip");
        // The own-host list wins over pool 1: the listed instruments each a worker of their own, the rest one process.
        const auto listed = run(Policy::pool1, twoOnABus, notes, {"org.kick.test.midi"}), reference = run(Policy::plugin, twoOnABus, notes);
        check(listed.processes == 3 && listed.lanes.at("i1") != listed.lanes.at("a1") && listed.lanes.at("i2") != listed.lanes.at("i1") && together(listed, {"a1", "b1", "a2", "b2", "bus1", "bus2"}) && listed.audio == reference.audio,
              "two on a bus, pool 1 with the instruments on the own-host list: they apart, everything else one process, as plugin sounds: " + std::to_string(listed.processes));
        Wide w2; place(Policy::chain); twoOnABus(w2); w2.graph.commit();
        auto lane = [&](const char* n) { return w2.graph.host(w2.nodes[n]).lane(); };
        check(lane("a1") == lane("b1") && lane("a2") == lane("b2") && lane("a1") != lane("a2") && lane("bus1") == lane("bus2") && lane("bus1") != lane("a1") && lane("bus1") != lane("a2")
              && w2.graph.grouped(w2.nodes["b1"]) && w2.graph.grouped(w2.nodes["b2"]) && w2.graph.grouped(w2.nodes["bus2"]) && !w2.graph.grouped(w2.nodes["bus1"]),
              "two on a bus, chain: each part's inserts one process and round trip, the bus its own, entered from both parts");
    }

    // chain: one part's chain process killed mid-play: only that part goes silent, and it comes back.
    void killOneChain(Policy policy = Policy::chain)
    {
        place(policy);
        Tines tines;
        Wide w; w.paced = true; tines.build(w); w.graph.commit();
        const auto pid = w.graph.host(w.nodes["p2chorus"]).workerPid();
        const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
        auto next = std::chrono::steady_clock::now();
        bool othersSilent = false, silent = false, back = false; std::string quiet; uint32_t silentAt = 0;
        for(uint32_t b = 0; b < 2000 && !back; ++b)
        {
            std::this_thread::sleep_until(next += period);
            std::vector<Event> e; // struck again every 100 blocks: a restart replays controllers, not notes
            if(b % 100 == 0) for(int p = 0; p < 5; ++p) e.push_back(Event::note(true, p * 24 + 12, .8));
            if(b % 100 == 98) for(int p = 0; p < 5; ++p) e.push_back(Event::note(false, p * 24 + 12, 0));
            w.play(b, e);
            if(b == 350 && pid > 0) kill(pid_t(pid), SIGKILL);
            const bool held = b % 100 > 10 && b % 100 < 90;
            if(b > 350 && held) for(const uint32_t port : {1u, 2u, 4u, 5u}) if(w.peak(port) < 1e-6f && !othersSilent) { othersSilent = true; quiet = "port " + std::to_string(port) + " at block " + std::to_string(b) + " peak " + std::to_string(w.peak(port)); }
            if(b > 350 && held && w.peak(3) < 1e-6f && !silent) { silent = true; silentAt = b; }
            if(b > 350 && silent && held && w.peak(3) > 1e-4f) back = true;
        }
        check(pid > 0 && silent, std::string(name(policy)) + " kill: part 3's chain process killed, and part 3 went silent");
        check(back, std::string(name(policy)) + " kill: part 3 came back (silent from block " + std::to_string(silentAt) + "; " + w.graph.host(w.nodes["p2chorus"]).failure() + ")");
        check(!othersSilent, std::string(name(policy)) + " kill: the other four parts never went silent (" + quiet + ")");
    }
}

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: %s WORKER NORMAL.clap [BASIC_SYNTH.clap]\n", argv[0]); return 2; }
    worker = argv[1]; normal = argv[2]; synth = argc > 3 ? argv[3] : nullptr;
    // The audio cores (realtime.h's audioThreadMask): a side's own list over the shared one; a slot one of its own, round-robin.
    using oscairn::worker::audioThreadMask;
    check(audioThreadMask("1-3", nullptr, -1) == 0b1110 && audioThreadMask("1-3", "", 2) == 0b1110 && audioThreadMask("0-3", "3", -1) == 0b1000
          && audioThreadMask(nullptr, "0-2", -1) == 0b111 && audioThreadMask(nullptr, "0-2", 0) == 0b1 && audioThreadMask(nullptr, "0,2", 1) == 0b100
          && audioThreadMask(nullptr, "0-2", 4) == 0b10 && audioThreadMask("1", "bad", 0) == 0 && audioThreadMask(nullptr, nullptr, 0) == 0,
          "audio cores: each side's list over the shared one, a worker process's slot one core round-robin, a bad list none");
    setenv("OSCAIRN_WORKER_BUDGET_US", "200000", 0); // unpaced and compared sample for sample: no block given up on a loaded machine
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
    if(synth) { policies(); killOneChain(); if(std::getenv("KILL_PLUGIN")) killOneChain(Policy::plugin); }
    if(failures) std::fprintf(stderr, "%d failure(s)\n", failures);
    else std::printf("plugin groups: every case as with groups off\n");
    return failures ? 1 : 0;
}
