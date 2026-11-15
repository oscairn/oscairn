// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Jacob Schooley and Oscairn contributors (MIT: core/clap-ext/LICENSE).
// A test fixture: oscairn-clap's sound-snapshots example at c2119db (examples/sound-contexts/sound_contexts.cpp
// built with OSCAIRN_EXAMPLE_SNAPSHOTS, as examples/sound-snapshots does), copied as it is: native sound contexts,
// ring-out, choke_retiring, and CLAP_PROCESS_SLEEP when no voice sounds. worker-sleep runs it in a worker.
#define OSCAIRN_EXAMPLE_ID "org.oscairn.example.sound-snapshots"
#define OSCAIRN_EXAMPLE_NAME "Oscairn Example Sound Snapshots"
#define OSCAIRN_EXAMPLE_SUMMARY "A snapshot's sounds prepared beside the playing ones (oscairn.sound-snapshots)"
#define OSCAIRN_EXAMPLE_SNAPSHOTS 1
//
// oscairn.sound-contexts (draft 5): up to four prepared sounds in one instance, each MIDI
// channel choosing one, held notes keeping theirs, no reload and no extra instance per sound.
// The sound is one sine with an attack/release, so a patch is two parameter values; the SDK's
// oscairn::contexts::Engine owns the slots, the selection queue, the voices and the snapshot.
// See docs/rfcs/0005-sound-contexts.md and plugins/basic-synth.
#include <clap/clap.h>
#include <oscairn-clap/sound-contexts-plugin.hpp>

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <istream>
#include <ostream>

namespace
{
    constexpr std::array<const char*, 2> names{"Level", "Tone"};
    constexpr std::array<double, 2> maximum{1, 1}, initial{.5, .5};

    // sound-snapshots/sound_snapshots.cpp includes this file with its own ID and one extra table.
#ifndef OSCAIRN_EXAMPLE_ID
#define OSCAIRN_EXAMPLE_ID "org.oscairn.example.sound-contexts"
#endif
#ifndef OSCAIRN_EXAMPLE_NAME
#define OSCAIRN_EXAMPLE_NAME "Oscairn Example Sound Contexts"
#endif
#ifndef OSCAIRN_EXAMPLE_SUMMARY
#define OSCAIRN_EXAMPLE_SUMMARY "Four prepared sounds in one instance (oscairn.sound-contexts)"
#endif

    const char* const features[] = {CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PLUGIN_FEATURE_SYNTHESIZER, CLAP_PLUGIN_FEATURE_STEREO, nullptr};
    const clap_plugin_descriptor_t descriptor{
        CLAP_VERSION_INIT, OSCAIRN_EXAMPLE_ID, OSCAIRN_EXAMPLE_NAME, "Oscairn", "", "", "", "0.1.0", OSCAIRN_EXAMPLE_SUMMARY, features};

    struct Patch
    {
        std::array<double, 2> values = {initial[0], initial[1]};
    };
    struct Voice : oscairn::contexts::VoiceBase
    {
        double phase = 0, level = 0;
    };

    struct Synth : oscairn::contexts::Engine<Synth, Voice, Patch, 4, 16>
    {
        double rate = 48000;
        std::array<std::atomic<double>, 2> values{initial[0], initial[1]}; // the main thread reads these while audio runs
        static constexpr const char* stateVersion = "oscairn-example-contexts-1";
        static constexpr size_t maxStateBytes = 4096;

        bool accepts(const Patch&, const clap_event_note_t&) { return true; }
        bool asleep() const { for(const auto& v : voices) if(v.active) return false; return true; }
        void start(Voice& v, Context&) { v.phase = v.level = 0; }
        void release(Voice& v) { v.held = false; }
        void render(const clap_process_t*, uint32_t from, uint32_t to);
        void applyParam(const clap_event_header_t*);
        void selectedGlobally(Context& next, const clap_output_events_t* out);
        static uint32_t prepare(const clap_plugin_t*, const char* name, const oscairn_sound_parameter_t*, uint32_t,
                               const char* patch, const oscairn_asset_ref_t*, char*, uint32_t);
        bool writePatch(std::ostream& out, const Patch& patch) { return bool(out << ' ' << patch.values[0] << ' ' << patch.values[1]); }
        bool readPatch(std::istream& in, Patch& patch)
        {
            double a = 0, b = 0;
            if(!(in >> a >> b) || !std::isfinite(a) || !std::isfinite(b) || a < 0 || a > maximum[0] || b < 0 || b > maximum[1]) return false;
            patch.values = {a, b};
            return true;
        }
        // The instance's own values follow the sound every channel now selects.
        void restored() { for(size_t i = 0; i < names.size(); ++i) values[i] = contexts[selected[0]].patch.values[i]; }
    };
    Synth& self(const clap_plugin_t* p) { return *static_cast<Synth*>(p->plugin_data); }

    void Synth::render(const clap_process_t* block, uint32_t from, uint32_t to)
    {
        auto& s = *this;
        std::array<double, contextCapacity> level{}; // per context slot
        std::array<bool, contextCapacity> copied{};
        std::array<float* const*, contextCapacity> ring{}; // per context: its ring-out channels, or none
        for(const auto& v : voices)
            if(v.active && !copied[v.context])
            {
                copied[v.context] = true;
                const auto& patch = contexts[v.context].patch;
                level[v.context] = patch.values[0];
                ring[v.context] = s.retiring(v.context); // null unless the host gave this process() a ring-out buffer
            }
        float* out[2]{block->audio_outputs[0].data32[0], block->audio_outputs[0].data32[1]};
        for(uint32_t i = from; i < to; ++i)
        {
            s.eventTime = i;
            double sum = 0;
            for(auto& v : voices)
            {
                if(!v.active) continue;
                // A 5 ms attack and release; a voice never outlives its own fade.
                const double step = 1.0 / (0.005 * rate);
                if(v.held) v.level = std::min(1.0, v.level + step);
                else v.level -= step;
                if(v.level <= 0) { finishVoice(v); continue; }
                const float sample = float(std::sin(v.phase) * v.level * v.velocity * level[v.context] * amplitude(v, i - from) * .3);
                // A retiring sound's main output goes to its two ring-out channels instead.
                if(const auto* channels = ring[v.context]) channels[0][i] += sample, channels[1][i] += sample;
                else sum += sample;
                v.phase = std::fmod(v.phase + 440.0 * 2 * M_PI / rate, 2 * M_PI);
            }
            out[0][i] = out[1][i] = float(sum);
        }
    }

    // A parameter edit on a channel lands on the sound that channel selects.
    void Synth::applyParam(const clap_event_header_t* header)
    {
        if(header->space_id != CLAP_CORE_EVENT_SPACE_ID || header->type != CLAP_EVENT_PARAM_VALUE) return;
        const auto& e = *reinterpret_cast<const clap_event_param_value_t*>(header);
        const auto id = e.param_id;
        if(id < 1 || id > names.size() || !std::isfinite(e.value) || e.channel < -1 || e.channel > 15) return;
        const double value = std::clamp(e.value, 0.0, maximum[id - 1]);
        for(int ch = 0; ch < 16; ++ch)
            if(e.channel < 0 || e.channel == ch) contexts[selected[ch]].patch.values[id - 1] = value;
    }

    void Synth::selectedGlobally(Context& next, const clap_output_events_t* output)
    {
        for(size_t i = 0; i < names.size(); ++i)
        {
            values[i] = next.patch.values[i];
            clap_event_param_value_t e{};
            e.header = {sizeof(e), 0, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE, 0};
            e.param_id = clap_id(i + 1);
            e.note_id = e.port_index = e.channel = e.key = -1;
            e.value = next.patch.values[i];
            if(output) output->try_push(output, &e.header);
        }
    }

    uint32_t Synth::prepare(const clap_plugin_t* p, const char* name, const oscairn_sound_parameter_t* supplied, uint32_t count,
                           const char* patch, const oscairn_asset_ref_t* base, char* message, uint32_t capacity)
    {
        if(patch || base) { if(message && capacity) std::snprintf(message, capacity, "this plugin takes no patch text"); return CLAP_INVALID_ID; }
        if(!name || std::strlen(name) > 63) return CLAP_INVALID_ID;
        Patch values;
        for(uint32_t i = 0; i < count; ++i)
        {
            const auto& v = supplied[i];
            if(v.id < 1 || v.id > names.size() || !std::isfinite(v.value) || v.value < 0 || v.value > maximum[v.id - 1])
            {
                if(message && capacity) std::snprintf(message, capacity, "value %u is not a parameter of this plugin", v.id);
                return CLAP_INVALID_ID;
            }
            values.values[v.id - 1] = v.value;
        }
        auto& s = self(p);
        auto* slot = s.freeSlot();
        if(!slot) { if(message && capacity) std::snprintf(message, capacity, "no free sound context"); return CLAP_INVALID_ID; }
        slot->patch = values;
        return s.publish(*slot, name);
    }

    const clap_plugin_params_t params{
        [](const clap_plugin_t*) -> uint32_t { return uint32_t(names.size()); },
        [](const clap_plugin_t*, uint32_t i, clap_param_info_t* out) -> bool
        {
            if(i >= names.size()) return false;
            *out = {};
            out->id = i + 1;
            out->flags = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_MODULATABLE;
            out->max_value = maximum[i];
            out->default_value = initial[i];
            std::snprintf(out->name, sizeof out->name, "%s", names[i]);
            return true;
        },
        [](const clap_plugin_t* p, clap_id id, double* v) -> bool
        {
            if(id < 1 || id > names.size()) return false;
            *v = self(p).values[id - 1];
            return true;
        },
        [](const clap_plugin_t*, clap_id id, double v, char* text, uint32_t size) -> bool
        {
            if(id < 1 || id > names.size() || !std::isfinite(v)) return false;
            std::snprintf(text, size, "%.0f%%", v * 100);
            return true;
        },
        [](const clap_plugin_t*, clap_id id, const char* text, double* v) -> bool
        {
            if(id < 1 || id > names.size()) return false;
            char* end = nullptr;
            const double parsed = std::strtod(text, &end);
            if(end == text || !std::isfinite(parsed) || parsed < 0 || parsed > maximum[id - 1]) return false;
            *v = parsed;
            return true;
        },
        [](const clap_plugin_t* p, const clap_input_events_t* in, const clap_output_events_t* out)
        {
            self(p).applySelections(out);
            for(uint32_t i = 0; in && i < in->size(in); ++i) self(p).applyParam(in->get(in, i));
        }};

    const clap_plugin_audio_ports_t audio{
        [](const clap_plugin_t*, bool input) -> uint32_t { return input ? 0u : 1u; },
        [](const clap_plugin_t*, uint32_t i, bool input, clap_audio_port_info_t* out) -> bool
        {
            if(i || input) return false;
            *out = {};
            out->id = 0;
            out->flags = CLAP_AUDIO_PORT_IS_MAIN;
            out->channel_count = 2;
            out->port_type = CLAP_PORT_STEREO;
            out->in_place_pair = CLAP_INVALID_ID;
            std::strcpy(out->name, "Main");
            return true;
        }};
    const clap_plugin_note_ports_t notes{
        [](const clap_plugin_t*, bool input) -> uint32_t { return input ? 1u : 0u; },
        [](const clap_plugin_t*, uint32_t i, bool input, clap_note_port_info_t* out) -> bool
        {
            if(i || !input) return false;
            *out = {};
            out->id = 0;
            out->supported_dialects = out->preferred_dialect = CLAP_NOTE_DIALECT_CLAP;
            std::strcpy(out->name, "Notes");
            return true;
        }};

    const clap_plugin_state_t snapshot{Synth::saveSnapshot, Synth::loadSnapshot};
    const oscairn_plugin_sound_contexts_t soundContexts{Synth::capacity, Synth::get, Synth::prepare, Synth::select, Synth::discard, &snapshot, nullptr};
#ifdef OSCAIRN_EXAMPLE_SNAPSHOTS
    // Instant Performance changes are one more table and one get_extension line (docs/plugin-porting.md,
    // "Instant Performance changes"): the SDK prepares a snapshot's sounds beside the playing ones.
    const oscairn_plugin_sound_snapshots_t soundSnapshots{Synth::prepareSnapshot, Synth::setRingOut, Synth::chokeRetiring, nullptr, nullptr};
#endif

    const clap_plugin_t* create()
    {
        auto* s = new Synth;
        s->plugin.desc = &descriptor;
        s->plugin.plugin_data = s;
        s->plugin.init = [](const clap_plugin_t*) { return true; };
        s->plugin.destroy = [](const clap_plugin_t* p) { delete &self(p); };
        s->plugin.activate = [](const clap_plugin_t* p, double rate, uint32_t, uint32_t)
        {
            if(!std::isfinite(rate) || rate < 1) return false;
            auto& s = self(p);
            s.rate = rate;
            s.setSampleRate(rate);
            s.resetVoices();
            return true;
        };
        s->plugin.deactivate = [](const clap_plugin_t*) {};
        s->plugin.start_processing = [](const clap_plugin_t*) { return true; };
        s->plugin.stop_processing = [](const clap_plugin_t*) {};
        s->plugin.reset = [](const clap_plugin_t* p) { self(p).resetVoices(); };
        s->plugin.process = Synth::process;
        s->plugin.on_main_thread = [](const clap_plugin_t*) {};
        s->plugin.get_extension = [](const clap_plugin_t*, const char* id) -> const void*
        {
            if(!std::strcmp(id, OSCAIRN_EXT_SOUND_CONTEXTS)) return &soundContexts;
#ifdef OSCAIRN_EXAMPLE_SNAPSHOTS
            if(!std::strcmp(id, OSCAIRN_EXT_SOUND_SNAPSHOTS)) return &soundSnapshots;
#endif
            if(!std::strcmp(id, CLAP_EXT_STATE)) return &snapshot;
            if(!std::strcmp(id, CLAP_EXT_AUDIO_PORTS)) return &audio;
            if(!std::strcmp(id, CLAP_EXT_NOTE_PORTS)) return &notes;
            if(!std::strcmp(id, CLAP_EXT_PARAMS)) return &params;
            return nullptr;
        };
        return &s->plugin;
    }
    const clap_plugin_factory_t factory{
        [](const clap_plugin_factory_t*) -> uint32_t { return 1; },
        [](const clap_plugin_factory_t*, uint32_t i) -> const clap_plugin_descriptor_t* { return i ? nullptr : &descriptor; },
        [](const clap_plugin_factory_t*, const clap_host_t*, const char* id) -> const clap_plugin_t*
        { return std::strcmp(id, descriptor.id) ? nullptr : create(); }};
}

extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{
    CLAP_VERSION_INIT, [](const char*) { return true; }, []() {},
    [](const char* id) -> const void* { return std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) ? nullptr : &factory; }};
