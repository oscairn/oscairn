// SPDX-License-Identifier: AGPL-3.0-or-later
// A test host for Oscairn.clap (RFC 0047 step 2, #294): it loads the built .clap through the CLAP C API,
// as a DAW does, and checks the descriptor and ports, activation at two sample rates, notes and MIDI at
// several block sizes with sound from a test library's Performance, state save and restore, and states that
// must be refused or kept.
//
//   oscairn-clap-shell-test Oscairn.clap basic-synth.clap
#include "oscairn/session.h"
#include "AllocationGuard.h"

#include <clap/clap.h>
#include <clap/ext/audio-ports.h>
#include <clap/ext/latency.h>
#include <clap/ext/note-ports.h>
#include <clap/ext/state.h>
#include <clap/ext/tail.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dlfcn.h>
#include <filesystem>
#include <string>
#include <thread>
#include <unistd.h>
#include <vector>

namespace fs = std::filesystem;

namespace
{
    constexpr uint32_t maxFrames = 4096;
    int failures = 0;

    void check(bool ok, const std::string& what)
    {
        if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; }
    }


    const clap_host_t host = {CLAP_VERSION, nullptr, "oscairn-clap-shell-test", "Oscairn", "", "1", [](const clap_host_t*, const char*) -> const void* { return nullptr; }, [](const clap_host_t*) {}, [](const clap_host_t*) {}, [](const clap_host_t*) {}};

    // The events of one block, in the order given (sorted by time).
    union Event
    {
        clap_event_header_t header;
        clap_event_note_t note;
        clap_event_midi_t midi;
    };
    Event noteEvent(uint16_t type, uint32_t time, int16_t key, double velocity, int16_t channel = 0)
    {
        Event e{};
        e.note = {{sizeof(clap_event_note_t), time, CLAP_CORE_EVENT_SPACE_ID, type, 0}, -1, 0, channel, key, velocity};
        return e;
    }
    Event midiEvent(uint32_t time, uint8_t a, uint8_t b, uint8_t c)
    {
        Event e{};
        e.midi = {{sizeof(clap_event_midi_t), time, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI, 0}, 0, {a, b, c}};
        return e;
    }

    struct Instance
    {
        const clap_plugin_t* plugin = nullptr;
        std::vector<float> left = std::vector<float>(maxFrames), right = std::vector<float>(maxFrames);

        // One block under the allocation guard: its peak, or -1 when the plugin refused it.
        float block(uint32_t frames, std::vector<Event> events = {})
        {
            clap_input_events_t in = {&events, [](const clap_input_events_t* l) { return uint32_t(static_cast<std::vector<Event>*>(l->ctx)->size()); },
                                      [](const clap_input_events_t* l, uint32_t i) -> const clap_event_header_t* { return &static_cast<std::vector<Event>*>(l->ctx)->at(i).header; }};
            clap_output_events_t out = {nullptr, [](const clap_output_events_t*, const clap_event_header_t*) { return false; }};
            float* channels[2] = {left.data(), right.data()};
            clap_audio_buffer_t buffer = {channels, nullptr, 2, 0, 0};
            clap_process_t process = {};
            process.frames_count = frames;
            process.audio_outputs = &buffer;
            process.audio_outputs_count = 1;
            process.in_events = &in;
            process.out_events = &out;
            forbidAllocation = true;
            const auto status = plugin->process(plugin, &process);
            forbidAllocation = false;
            if(status == CLAP_PROCESS_ERROR) return -1;
            float peak = 0;
            for(uint32_t i = 0; i < frames; ++i) peak = std::max({peak, std::abs(left[i]), std::abs(right[i])});
            return peak;
        }

        // Blocks while a load runs, a note struck in each until one sounds: whether it did within a minute.
        bool settle()
        {
            const auto until = std::chrono::steady_clock::now() + std::chrono::seconds(60);
            while(std::chrono::steady_clock::now() < until)
            {
                if(block(128, {noteEvent(CLAP_EVENT_NOTE_ON, 0, 60, 0.8)}) > 0.01f) { ringOut(60); return true; }
                std::this_thread::sleep_for(std::chrono::milliseconds(2));
            }
            return false;
        }

        void ringOut(int16_t key)
        {
            block(128, {noteEvent(CLAP_EVENT_NOTE_OFF, 0, key, 0.5)});
            for(int i = 0; i < 100; ++i) block(480);
        }

        // A note over blocks of these sizes, struck at the last frame of the first: the loudest the blocks were.
        float play(const std::vector<uint32_t>& sizes, bool asMidi, int16_t key = 60)
        {
            float peak = 0;
            bool played = true;
            for(size_t i = 0; i < sizes.size(); ++i)
            {
                std::vector<Event> events;
                if(i == 0) events.push_back(asMidi ? midiEvent(sizes[0] - 1, 0x90, uint8_t(key), 100) : noteEvent(CLAP_EVENT_NOTE_ON, sizes[0] - 1, key, 0.8));
                const auto p = block(sizes[i], events);
                played = played && p >= 0;
                peak = std::max(peak, p);
            }
            check(played, "every block of the sequence is processed");
            ringOut(key);
            return peak;
        }

        std::vector<uint8_t> save(bool* ok = nullptr)
        {
            const auto* state = static_cast<const clap_plugin_state_t*>(plugin->get_extension(plugin, CLAP_EXT_STATE));
            std::vector<uint8_t> bytes;
            clap_ostream_t stream = {&bytes, [](const clap_ostream_t* s, const void* data, uint64_t size) -> int64_t {
                                         auto& v = *static_cast<std::vector<uint8_t>*>(s->ctx);
                                         const auto n = std::min<uint64_t>(size, size / 2 + 1); // part of it at a time
                                         v.insert(v.end(), static_cast<const uint8_t*>(data), static_cast<const uint8_t*>(data) + n);
                                         return int64_t(n);
                                     }};
            const bool saved = state->save(plugin, &stream);
            if(ok) *ok = saved;
            return bytes;
        }

        bool load(const std::vector<uint8_t>& bytes)
        {
            const auto* state = static_cast<const clap_plugin_state_t*>(plugin->get_extension(plugin, CLAP_EXT_STATE));
            struct Source { const std::vector<uint8_t>* bytes; size_t at; } source{&bytes, 0};
            clap_istream_t stream = {&source, [](const clap_istream_t* s, void* buffer, uint64_t size) -> int64_t {
                                         auto& src = *static_cast<Source*>(s->ctx);
                                         const auto n = std::min<uint64_t>({size, 1000, src.bytes->size() - src.at}); // a thousand bytes at a time
                                         std::memcpy(buffer, src.bytes->data() + src.at, n);
                                         src.at += n;
                                         return int64_t(n);
                                     }};
            return state->load(plugin, &stream);
        }
    };

    struct Module
    {
        void* handle = nullptr;
        const clap_plugin_entry_t* entry = nullptr;
        const clap_plugin_factory_t* factory = nullptr;
    };

    Module open(const fs::path& path)
    {
        Module m;
        auto binary = path;
        if(fs::is_directory(path)) binary = path / "Contents/MacOS" / path.stem(); // a macOS bundle
        m.handle = dlopen(binary.c_str(), RTLD_NOW | RTLD_LOCAL);
        if(!m.handle) { std::fprintf(stderr, "dlopen: %s\n", dlerror()); return m; }
        m.entry = static_cast<const clap_plugin_entry_t*>(dlsym(m.handle, "clap_entry"));
        if(m.entry && clap_version_is_compatible(m.entry->clap_version) && m.entry->init(path.c_str()))
            m.factory = static_cast<const clap_plugin_factory_t*>(m.entry->get_factory(CLAP_PLUGIN_FACTORY_ID));
        return m;
    }

    // A plugin made and initialized, as the host does; null when it did not.
    bool create(const Module& m, Instance& i, const char* id = "org.oscairn.oscairn")
    {
        i.plugin = m.factory->create_plugin(m.factory, &host, id);
        if(!i.plugin) return false;
        if(!i.plugin->init(i.plugin)) { i.plugin->destroy(i.plugin); i.plugin = nullptr; return false; }
        return true;
    }

    void destroy(Instance& i) { i.plugin->destroy(i.plugin); i.plugin = nullptr; }

    bool start(Instance& i, double rate, uint32_t frames = maxFrames)
    {
        return i.plugin->activate(i.plugin, rate, 1, frames) && i.plugin->start_processing(i.plugin);
    }

    void stop(Instance& i)
    {
        i.plugin->stop_processing(i.plugin);
        i.plugin->deactivate(i.plugin);
    }

    bool contains(const std::vector<uint8_t>& bytes, const std::string& text)
    {
        return std::search(bytes.begin(), bytes.end(), text.begin(), text.end()) != bytes.end();
    }
}

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: oscairn-clap-shell-test Oscairn.clap basic-synth.clap\n"); return 2; }
    const std::string clapPath = argv[1], synthPath = argv[2];
    const auto home = fs::temp_directory_path() / ("oscairn-clap-shell-test-" + std::to_string(getpid()));
    fs::remove_all(home);
    fs::create_directories(home);
    setenv("OSCAIRN_DATA_HOME", home.c_str(), 1); // the library the test builds, not the user's
    setenv("OSCAIRN_BUNDLED_PLUGINS", fs::path(synthPath).parent_path().c_str(), 1);

    // The library: the Basic Synth alone (as plugin-core's test builds it).
    std::string performance;
    {
        oscairn::Session s(48000, 512);
        const auto part = s.addPart("Synth");
        s.insertIntoPart(part, 0, s.add("synth", synthPath));
        s.commit();
        performance = s.savePerformanceToLibrary("CLAP Synth", true);
    }

    const auto module = open(clapPath);
    check(module.factory != nullptr, "the .clap loads and has a plugin factory");
    if(!module.factory) return 1;

    check(module.factory->get_plugin_count(module.factory) == 1, "the factory has one plugin");
    const auto* d = module.factory->get_plugin_descriptor(module.factory, 0);
    check(d && std::string(d->id) == "org.oscairn.oscairn" && std::string(d->name) == "Oscairn", "its id and name");
    auto hasFeature = [&](const char* f) { for(const char* const* p = d->features; p && *p; ++p) if(!std::strcmp(*p, f)) return true; return false; };
    check(d && hasFeature(CLAP_PLUGIN_FEATURE_INSTRUMENT) && hasFeature(CLAP_PLUGIN_FEATURE_SAMPLER) && hasFeature(CLAP_PLUGIN_FEATURE_SYNTHESIZER) && hasFeature(CLAP_PLUGIN_FEATURE_STEREO), "its features: instrument, sampler, synthesizer, stereo");
    check(module.factory->get_plugin_descriptor(module.factory, 1) == nullptr, "no second descriptor");
    check(module.factory->create_plugin(module.factory, &host, "org.example.other") == nullptr, "an ID that is not its own makes nothing");

    // Picked by the environment (temporary, until the editor), at init.
    setenv("OSCAIRN_CLAP_PERFORMANCE", performance.c_str(), 1);
    Instance a;
    check(create(module, a), "an instance is created and initialized");
    if(!a.plugin) return 1;
    unsetenv("OSCAIRN_CLAP_PERFORMANCE");

    const auto* audio = static_cast<const clap_plugin_audio_ports_t*>(a.plugin->get_extension(a.plugin, CLAP_EXT_AUDIO_PORTS));
    const auto* notes = static_cast<const clap_plugin_note_ports_t*>(a.plugin->get_extension(a.plugin, CLAP_EXT_NOTE_PORTS));
    check(audio && audio->count(a.plugin, false) == 1 && audio->count(a.plugin, true) == 0, "one audio output, no input");
    clap_audio_port_info_t port;
    check(audio && audio->get(a.plugin, 0, false, &port) && port.channel_count == 2 && (port.flags & CLAP_AUDIO_PORT_IS_MAIN) && std::string(port.port_type) == CLAP_PORT_STEREO, "the output is the main stereo port");
    check(audio && !audio->get(a.plugin, 1, false, &port), "no second audio output");
    clap_note_port_info_t notePort;
    check(notes && notes->count(a.plugin, true) == 1 && notes->count(a.plugin, false) == 0, "one note input, no note output");
    check(notes && notes->get(a.plugin, 0, true, &notePort) && (notePort.supported_dialects & CLAP_NOTE_DIALECT_CLAP) && (notePort.supported_dialects & CLAP_NOTE_DIALECT_MIDI), "it takes CLAP and MIDI notes");
    const auto* latency = static_cast<const clap_plugin_latency_t*>(a.plugin->get_extension(a.plugin, CLAP_EXT_LATENCY));
    const auto* tail = static_cast<const clap_plugin_tail_t*>(a.plugin->get_extension(a.plugin, CLAP_EXT_TAIL));
    check(latency && latency->get(a.plugin) == 0, "latency 0");
    check(tail && tail->get(a.plugin) == UINT32_MAX, "an infinite tail");
    check(a.plugin->get_extension(a.plugin, CLAP_EXT_STATE) != nullptr && a.plugin->get_extension(a.plugin, "clap.params") == nullptr, "state, and no parameters yet");

    // Two sample rates, blocks of several sizes with notes as CLAP events and as MIDI.
    for(const double rate : {44100.0, 48000.0})
    {
        check(start(a, rate), "activates and starts at " + std::to_string(int(rate)));
        check(a.settle(), "the Performance picked at init loads and sounds at " + std::to_string(int(rate)));
        for(const auto& sizes : std::vector<std::vector<uint32_t>>{std::vector<uint32_t>(4800, 1), std::vector<uint32_t>(75, 64), std::vector<uint32_t>(38, 128),
                                                                   std::vector<uint32_t>(10, 480), {1, 7, 512, 33, 480, 64, 2, 300, 128, 511, 17, 256, 1, 90, 512}, {4096, 4096}, {3000, 1100, 1}})
            for(const bool asMidi : {false, true})
                check(a.play(sizes, asMidi) > 0.01f, "notes sound at blocks of " + std::to_string(sizes[0]) + (sizes.size() > 1 && sizes[1] != sizes[0] ? " and varying" : "") + (asMidi ? " as MIDI" : " as CLAP events") + " at " + std::to_string(int(rate)));
        stop(a);
    }

    // The rest of MIDI: controllers, bend, both pressures and the sustain pedal go in beside notes without harm.
    check(start(a, 48000), "activates again");
    check(a.block(512, {midiEvent(0, 0xb0, 1, 64), midiEvent(10, 0xe0, 0, 96), midiEvent(20, 0xd0, 80, 0), midiEvent(30, 0xb0, 64, 127), midiEvent(40, 0x90, 64, 100),
                        midiEvent(50, 0xa0, 64, 70), midiEvent(60, 0x80, 64, 0), midiEvent(70, 0xb0, 64, 0), midiEvent(80, 0xc0, 3, 0),
                        noteEvent(CLAP_EVENT_NOTE_CHOKE, 90, 64, 0), noteEvent(CLAP_EVENT_NOTE_OFF, 100, -1, 0), midiEvent(110, 0xf8, 0, 0)}) >= 0, "controllers, bend, pressure and sustain are taken");
    check(a.block(0) >= 0 && a.block(512, {noteEvent(CLAP_EVENT_NOTE_ON, 100000, 60, 0.8)}) >= 0, "an empty block, and an event past the block's end, are taken");
    check(a.play({128, 128}, false) > 0.01f, "and it still sounds");

    // State: save, destroy, restore in a new instance (no pick in the environment) and get the Performance.
    bool saved = false;
    const auto state = a.save(&saved);
    check(saved && state.size() > 8 && std::memcmp(state.data(), "OSPS", 4) == 0 && contains(state, performance) && contains(state, "CLAP Synth"), "the state is the core's, with the Performance's identity");
    stop(a);
    destroy(a);

    Instance b;
    check(create(module, b), "a second instance is created");
    check(b.load(state), "it takes the saved state");
    check(start(b, 44100), "activates at once after the state, while it loads"); // CLAP hosts activate in either order
    check(b.settle(), "and plays the same Performance");
    saved = false;
    const auto again = b.save(&saved);
    check(saved && contains(again, performance) && contains(again, "CLAP Synth"), "its own save names the same Performance");

    // Refused: states that are not ours change nothing and do not crash.
    check(!b.load({}) && !b.load({1, 2, 3}), "an empty state and bytes that are no state are refused");
    auto newer = state;
    newer[4] = 2;
    check(!b.load(newer), "a state of a later version is refused");
    check(!b.load(std::vector<uint8_t>(state.begin(), state.begin() + long(state.size() / 2))), "a truncated state is refused");
    std::vector<uint8_t> noise(100000);
    for(size_t i = 0; i < noise.size(); ++i) noise[i] = uint8_t((i * 2654435761u) >> 13);
    check(!b.load(noise), "noise is refused");
    auto header = state;
    header.resize(32);
    header[8] = 0xff, header[9] = 0xff, header[10] = 0xff, header[11] = 0x7f; // a string far longer than the state
    check(!b.load(header), "a state whose lengths run past its end is refused");
    check(b.play({128, 128, 128}, false) > 0.01f, "the Performance plays on after them");

    // A state that is ours but whose Performance does not open: the one before plays on and the project's state
    // comes back as it came, so saving the project loses nothing.
    auto broken = state;
    const std::string from = "\"version\":13", to = "\"version\":99";
    const auto at = std::search(broken.begin(), broken.end(), from.begin(), from.end());
    check(at != broken.end(), "the state holds the Performance's document");
    if(at != broken.end()) std::copy(to.begin(), to.end(), at);
    check(b.load(broken), "a state whose Performance will not open is taken (it fails in the load)");
    for(int i = 0; i < 400; ++i) { b.block(128); std::this_thread::sleep_for(std::chrono::milliseconds(2)); } // the load runs and fails
    check(b.save() == broken, "its state comes back byte for byte");
    check(b.play({128, 128, 128}, false) > 0.01f, "and the Performance before it still plays");
    stop(b);
    destroy(b);

    // Destroyed while its load is running, and a plugin that never activated.
    setenv("OSCAIRN_CLAP_PERFORMANCE", performance.c_str(), 1);
    Instance c;
    check(create(module, c), "a third instance is created");
    destroy(c);
    Instance e;
    check(create(module, e), "a fourth instance is created");
    destroy(e);

    module.entry->deinit();
    dlclose(module.handle);
    fs::remove_all(home);
    if(failures) { std::fprintf(stderr, "%d failures\n", failures); return 1; }
    std::puts("oscairn.clap: all checks passed");
    return 0;
}
