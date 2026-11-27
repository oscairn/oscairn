// SPDX-License-Identifier: AGPL-3.0-or-later
// The DAW plugin as CLAP (RFC 0047 step 2, #294): Oscairn.clap, a thin shell over one plugin-core Instance.
//
// Choices where CLAP leaves them open:
//  - latency 0; tail infinite (the core cannot say how long a Performance's plugins ring on, so the host keeps
//    calling process()).
//  - The main thread does not wait for a load: load_state and activate (while a load runs) return at once.
//    State save waits for the control thread, except while a load runs: it then writes the state that load
//    came from. init waits for the engine to start; destroy for a load in progress to end.
//  - Notes are played as MIDI by channel, as on the device; note expressions are not used yet.
//  - Temporary, until the GUI (#304): OSCAIRN_CLAP_PERFORMANCE (a library ID) names the Performance picked at
//    init, and a state loaded after that replaces it. OSCAIRN_BUNDLED_PLUGINS is the plugin folder
//    (Config::bundledPlugins).
#include "oscairn/plugin_core.h"

#include <clap/clap.h>
#include <clap/ext/audio-ports.h>
#include <clap/ext/latency.h>
#include <clap/ext/note-ports.h>
#include <clap/ext/state.h>
#include <clap/ext/tail.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <vector>

namespace
{
    using namespace oscairn::plugin;

    // The most frames one core block takes: a longer host block is played in slices of it.
    constexpr uint32_t sliceFrames = 1024;
    constexpr size_t maxEvents = 256; // as many as the core plays

    const char* const features[] = {CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PLUGIN_FEATURE_SAMPLER, CLAP_PLUGIN_FEATURE_SYNTHESIZER, CLAP_PLUGIN_FEATURE_STEREO, nullptr};
    const clap_plugin_descriptor_t descriptor = {CLAP_VERSION, "org.oscairn.oscairn", "Oscairn", "Oscairn", "https://oscairn.org", "", "", "0.1.0",
                                                 "One Oscairn Performance as an instrument", features};

    struct Shell
    {
        clap_plugin_t plugin{};
        std::unique_ptr<Instance> instance;
        std::vector<uint8_t> loading;              // main thread: the state a load in progress came from
        std::array<MidiEvent, maxEvents> events{}; // audio thread: the block's events, then a slice's
        std::array<MidiEvent, maxEvents> slice{};
    };

    Shell& shell(const clap_plugin_t* p) { return *static_cast<Shell*>(p->plugin_data); }

    uint8_t data7(double v, int least) { return uint8_t(std::clamp(int(std::lround(v * 127)), least, 127)); }

    // A CLAP event as MIDI: false for the ones the core does not play.
    bool toMidi(const clap_event_header_t* h, uint32_t frames, MidiEvent& m) noexcept
    {
        if(h->space_id != CLAP_CORE_EVENT_SPACE_ID) return false;
        m.frame = std::min(h->time, frames - 1);
        switch(h->type)
        {
        case CLAP_EVENT_NOTE_ON:
        case CLAP_EVENT_NOTE_OFF:
        case CLAP_EVENT_NOTE_CHOKE:
        {
            const auto& n = *reinterpret_cast<const clap_event_note_t*>(h);
            const uint8_t channel = n.channel < 0 ? 0 : uint8_t(n.channel & 15);
            const bool on = h->type == CLAP_EVENT_NOTE_ON;
            m.size = 3;
            if(n.key < 0) // a wildcard note off: all notes off
            {
                if(on) return false;
                m.bytes[0] = uint8_t(0xb0 | channel), m.bytes[1] = 123, m.bytes[2] = 0;
            }
            else m.bytes[0] = uint8_t((on ? 0x90 : 0x80) | channel), m.bytes[1] = uint8_t(n.key & 127), m.bytes[2] = on ? data7(n.velocity, 1) : data7(n.velocity, 0);
            return true;
        }
        case CLAP_EVENT_MIDI:
        {
            const auto& e = *reinterpret_cast<const clap_event_midi_t*>(h);
            if(e.data[0] < 0x80 || e.data[0] >= 0xf0) return false; // no running status, no system messages
            m.size = (e.data[0] & 0xf0) == 0xc0 || (e.data[0] & 0xf0) == 0xd0 ? 2 : 3;
            for(int i = 0; i < 3; ++i) m.bytes[i] = e.data[i];
            return true;
        }
        default: return false;
        }
    }

    bool init(const clap_plugin_t* p)
    {
        auto& s = shell(p);
        try
        {
            Config config;
            config.maxFrames = sliceFrames;
            if(const char* bundled = std::getenv("OSCAIRN_BUNDLED_PLUGINS")) config.bundledPlugins = bundled;
            s.instance = std::make_unique<Instance>(config);
            if(const char* id = std::getenv("OSCAIRN_CLAP_PERFORMANCE"); id && *id) s.instance->pick(id);
            return true;
        }
        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: %s\n", e.what()); return false; }
    }

    void destroy(const clap_plugin_t* p) { delete &shell(p); }

    bool activate(const clap_plugin_t* p, double sampleRate, uint32_t, uint32_t)
    {
        auto& s = shell(p);
        if(!s.instance) return false;
        try { s.instance->activate(sampleRate); return true; }
        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: %s\n", e.what()); return false; }
    }

    void deactivate(const clap_plugin_t* p) { if(auto& s = shell(p); s.instance) s.instance->deactivate(); }
    bool startProcessing(const clap_plugin_t*) { return true; }
    void stopProcessing(const clap_plugin_t*) {}
    void reset(const clap_plugin_t*) {}
    void onMainThread(const clap_plugin_t*) {}

    clap_process_status process(const clap_plugin_t* p, const clap_process_t* in)
    {
        auto& s = shell(p);
        if(!s.instance || !in || in->audio_outputs_count < 1 || in->audio_outputs[0].channel_count < 2 || !in->audio_outputs[0].data32) return CLAP_PROCESS_ERROR;
        const auto frames = in->frames_count;
        auto** out = in->audio_outputs[0].data32;
        size_t count = 0;
        if(in->in_events && in->in_events->size && in->in_events->get)
            for(uint32_t i = 0, n = in->in_events->size(in->in_events); i < n && count < s.events.size(); ++i)
                if(const auto* h = in->in_events->get(in->in_events, i); h && frames && toMidi(h, frames, s.events[count])) ++count;
        // Events come sorted by time (CLAP); each slice takes those inside it.
        size_t next = 0;
        for(uint32_t start = 0; start < frames; start += sliceFrames)
        {
            const auto n = std::min(sliceFrames, frames - start);
            size_t inSlice = 0;
            for(; next < count && s.events[next].frame < start + n; ++next)
            {
                s.slice[inSlice] = s.events[next];
                s.slice[inSlice++].frame = s.events[next].frame - start;
            }
            Block block;
            block.frames = n;
            block.events = s.slice.data();
            block.eventCount = uint32_t(inSlice);
            block.main[0] = out[0] + start;
            block.main[1] = out[1] + start;
            s.instance->process(block); // false (silence written): not active yet, behind a load that began before activate
        }
        return CLAP_PROCESS_CONTINUE;
    }

    uint32_t audioPortCount(const clap_plugin_t*, bool isInput) { return isInput ? 0 : 1; }
    bool audioPortGet(const clap_plugin_t*, uint32_t index, bool isInput, clap_audio_port_info_t* info)
    {
        if(isInput || index != 0) return false;
        *info = {};
        info->id = 0;
        std::strcpy(info->name, "Main");
        info->flags = CLAP_AUDIO_PORT_IS_MAIN;
        info->channel_count = 2;
        info->port_type = CLAP_PORT_STEREO;
        info->in_place_pair = CLAP_INVALID_ID;
        return true;
    }
    const clap_plugin_audio_ports_t audioPorts = {audioPortCount, audioPortGet};

    uint32_t notePortCount(const clap_plugin_t*, bool isInput) { return isInput ? 1 : 0; }
    bool notePortGet(const clap_plugin_t*, uint32_t index, bool isInput, clap_note_port_info_t* info)
    {
        if(!isInput || index != 0) return false;
        *info = {};
        info->id = 0;
        info->supported_dialects = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI;
        info->preferred_dialect = CLAP_NOTE_DIALECT_MIDI; // the core plays MIDI by channel
        std::strcpy(info->name, "Notes");
        return true;
    }
    const clap_plugin_note_ports_t notePorts = {notePortCount, notePortGet};

    uint32_t latency(const clap_plugin_t*) { return 0; }
    const clap_plugin_latency_t latencyExt = {latency};
    uint32_t tail(const clap_plugin_t*) { return UINT32_MAX; }
    const clap_plugin_tail_t tailExt = {tail};

    bool stateSave(const clap_plugin_t* p, const clap_ostream_t* stream)
    {
        auto& s = shell(p);
        if(!s.instance) return false;
        try
        {
            // While a load runs the control thread is busy with it: the state it came from is what this one would hold.
            const auto bytes = s.instance->status().load == Load::Loading && !s.loading.empty() ? s.loading : s.instance->state();
            for(size_t at = 0; at < bytes.size();)
            {
                const auto wrote = stream->write(stream, bytes.data() + at, bytes.size() - at);
                if(wrote <= 0) return false;
                at += size_t(wrote);
            }
            return true;
        }
        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: %s\n", e.what()); return false; }
    }

    bool stateLoad(const clap_plugin_t* p, const clap_istream_t* stream)
    {
        auto& s = shell(p);
        if(!s.instance) return false;
        std::vector<uint8_t> bytes;
        uint8_t chunk[65536];
        for(int64_t got; (got = stream->read(stream, chunk, sizeof chunk)) != 0;)
        {
            if(got < 0) return false;
            bytes.insert(bytes.end(), chunk, chunk + got);
        }
        try { s.instance->setState(bytes); } // returns at once; bytes that are not a state throw, and nothing changes
        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: %s\n", e.what()); return false; }
        s.loading = std::move(bytes);
        return true;
    }
    const clap_plugin_state_t stateExt = {stateSave, stateLoad};

    const void* extension(const clap_plugin_t*, const char* id)
    {
        if(!std::strcmp(id, CLAP_EXT_AUDIO_PORTS)) return &audioPorts;
        if(!std::strcmp(id, CLAP_EXT_NOTE_PORTS)) return &notePorts;
        if(!std::strcmp(id, CLAP_EXT_STATE)) return &stateExt;
        if(!std::strcmp(id, CLAP_EXT_LATENCY)) return &latencyExt;
        if(!std::strcmp(id, CLAP_EXT_TAIL)) return &tailExt;
        return nullptr;
    }

    uint32_t factoryCount(const clap_plugin_factory_t*) { return 1; }
    const clap_plugin_descriptor_t* factoryDescriptor(const clap_plugin_factory_t*, uint32_t index) { return index == 0 ? &descriptor : nullptr; }
    const clap_plugin_t* factoryCreate(const clap_plugin_factory_t*, const clap_host_t*, const char* id)
    {
        if(!id || std::strcmp(id, descriptor.id)) return nullptr;
        auto* s = new Shell;
        s->plugin = {&descriptor, s, init, destroy, activate, deactivate, startProcessing, stopProcessing, reset, process, extension, onMainThread};
        return &s->plugin;
    }
    const clap_plugin_factory_t factory = {factoryCount, factoryDescriptor, factoryCreate};
}

extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry = {
    CLAP_VERSION,
    [](const char*) { return true; },
    [] {},
    [](const char* id) -> const void* { return id && !std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) ? &factory : nullptr; }};
