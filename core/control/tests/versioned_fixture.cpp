// SPDX-License-Identifier: AGPL-3.0-or-later
//
// One plugin in several versions, for moving saved sounds to a newer version (RFC 0043):
// org.oscairn.test.versioned, an instrument playing its Level (parameter 1) while a key is held.
// FIXTURE_VERSION names its version. Version 1 saves "V1 <level>"; a later version saves
// "V2 <level> <drive>", Drive (parameter 2) new in it, and reads version 1's state with Drive 0,
// which plays as version 1 did. FIXTURE_STRICT: it refuses any state but its own. FIXTURE_MONO: its
// output is one mono port, other ports than version 1's.
#include <clap/clap.h>
#include <clap/ext/audio-ports.h>
#include <clap/ext/note-ports.h>
#include <clap/ext/params.h>
#include <clap/ext/state.h>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <string>

#ifndef FIXTURE_VERSION
#define FIXTURE_VERSION "1.0.0"
#endif

namespace
{
    constexpr bool first = FIXTURE_VERSION[0] == '1';
#ifdef FIXTURE_STRICT
    constexpr bool strict = true;
#else
    constexpr bool strict = false;
#endif
#ifdef FIXTURE_MONO
    constexpr uint32_t channels = 1;
#else
    constexpr uint32_t channels = 2;
#endif
    const char* features[]{CLAP_PLUGIN_FEATURE_INSTRUMENT, nullptr};
    const clap_plugin_descriptor_t descriptor{CLAP_VERSION, "org.oscairn.test.versioned", "Versioned", "Oscairn", "", "", "", FIXTURE_VERSION, "", features};
    struct Plugin
    {
        clap_plugin_t api{};
        std::atomic<double> level{.5}, drive{0};
        bool gate = false;
        static Plugin& get(const clap_plugin_t* p) { return *static_cast<Plugin*>(p->plugin_data); }
        static void events(const clap_plugin_t* p, const clap_input_events_t* in, const clap_output_events_t*)
        {
            auto& s = get(p);
            for(uint32_t i = 0; i < in->size(in); ++i)
            {
                const auto* e = in->get(in, i);
                if(e->type == CLAP_EVENT_NOTE_ON) s.gate = true;
                if(e->type == CLAP_EVENT_NOTE_OFF) s.gate = false;
                if(e->type == CLAP_EVENT_PARAM_VALUE)
                {
                    const auto* v = reinterpret_cast<const clap_event_param_value_t*>(e);
                    if(v->param_id == 1) s.level = v->value;
                    if(v->param_id == 2 && !first) s.drive = v->value;
                }
            }
        }
        Plugin()
        {
            api.desc = &descriptor; api.plugin_data = this;
            api.init = [](const clap_plugin_t*) { return true; };
            api.destroy = [](const clap_plugin_t* p) { delete &get(p); };
            api.activate = [](const clap_plugin_t*, double, uint32_t, uint32_t) { return true; };
            api.deactivate = [](const clap_plugin_t*) {};
            api.start_processing = [](const clap_plugin_t*) { return true; };
            api.stop_processing = [](const clap_plugin_t*) {};
            api.reset = [](const clap_plugin_t* p) { get(p).gate = false; };
            api.on_main_thread = [](const clap_plugin_t*) {};
            api.process = [](const clap_plugin_t* p, const clap_process_t* b) -> clap_process_status {
                events(p, b->in_events, b->out_events);
                auto& s = get(p);
                const float out = s.gate ? float(s.level.load() * (1 + s.drive.load())) : 0.f;
                for(uint32_t c = 0; c < channels; ++c) for(uint32_t i = 0; i < b->frames_count; ++i) b->audio_outputs[0].data32[c][i] = out;
                return CLAP_PROCESS_CONTINUE;
            };
            api.get_extension = [](const clap_plugin_t*, const char* id) -> const void* {
                static const clap_plugin_audio_ports_t ports{
                    [](const clap_plugin_t*, bool in) -> uint32_t { return in ? 0 : 1; },
                    [](const clap_plugin_t*, uint32_t i, bool in, clap_audio_port_info_t* p) {
                        if(i || in) return false;
                        *p = {}; p->id = 0; std::strcpy(p->name, "out"); p->channel_count = channels;
                        p->flags = CLAP_AUDIO_PORT_IS_MAIN; p->port_type = channels == 2 ? CLAP_PORT_STEREO : CLAP_PORT_MONO; p->in_place_pair = CLAP_INVALID_ID; return true;
                    }};
                static const clap_plugin_note_ports_t notes{
                    [](const clap_plugin_t*, bool in) -> uint32_t { return in ? 1 : 0; },
                    [](const clap_plugin_t*, uint32_t i, bool in, clap_note_port_info_t* p) {
                        if(i || !in) return false;
                        *p = {}; p->id = 0; p->supported_dialects = p->preferred_dialect = CLAP_NOTE_DIALECT_CLAP; return true;
                    }};
                static const clap_plugin_params_t params{
                    [](const clap_plugin_t*) -> uint32_t { return first ? 1 : 2; },
                    [](const clap_plugin_t*, uint32_t i, clap_param_info_t* p) {
                        if(i >= (first ? 1u : 2u)) return false;
                        *p = {}; p->id = i + 1; std::strcpy(p->name, i ? "Drive" : "Level"); p->flags = CLAP_PARAM_IS_AUTOMATABLE;
                        p->min_value = 0; p->max_value = 1; p->default_value = i ? 0 : .5; return true;
                    },
                    [](const clap_plugin_t* p, clap_id id, double* v) {
                        if(id == 1) { *v = get(p).level; return true; }
                        if(id == 2 && !first) { *v = get(p).drive; return true; }
                        return false;
                    },
                    [](const clap_plugin_t*, clap_id, double v, char* out, uint32_t n) { std::snprintf(out, n, "%.3f", v); return true; },
                    nullptr, events};
                static const clap_plugin_state_t state{
                    [](const clap_plugin_t* p, const clap_ostream_t* s) {
                        char text[64];
                        const int n = first ? std::snprintf(text, sizeof text, "V1 %.17g", get(p).level.load()) : std::snprintf(text, sizeof text, "V2 %.17g %.17g", get(p).level.load(), get(p).drive.load());
                        return s->write(s, text, uint64_t(n)) == n;
                    },
                    [](const clap_plugin_t* p, const clap_istream_t* s) {
                        char text[64] = {};
                        int64_t got = 0, n;
                        while(got < 63 && (n = s->read(s, text + got, uint64_t(63 - got))) > 0) got += n;
                        double level = 0, drive = 0;
                        if(std::sscanf(text, "V2 %lf %lf", &level, &drive) == 2 && !first) {}
                        else if(std::sscanf(text, "V1 %lf", &level) == 1 && (first || !strict)) drive = 0; // version 1's state, as it played
                        else return false;
                        get(p).level = level; get(p).drive = drive;
                        return true;
                    }};
                if(!std::strcmp(id, CLAP_EXT_AUDIO_PORTS)) return &ports;
                if(!std::strcmp(id, CLAP_EXT_NOTE_PORTS)) return &notes;
                if(!std::strcmp(id, CLAP_EXT_PARAMS)) return &params;
                if(!std::strcmp(id, CLAP_EXT_STATE)) return &state;
                return nullptr;
            };
        }
    };
    const clap_plugin_factory_t factory{
        [](const clap_plugin_factory_t*) -> uint32_t { return 1; },
        [](const clap_plugin_factory_t*, uint32_t i) { return i ? nullptr : &descriptor; },
        [](const clap_plugin_factory_t*, const clap_host_t*, const char* id) -> const clap_plugin_t* {
            if(std::strcmp(id, descriptor.id)) return nullptr;
            return &(new Plugin)->api;
        }};
}
extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{
    CLAP_VERSION, [](const char*) { return true; }, []() {},
    [](const char* id) -> const void* { return !std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) ? &factory : nullptr; }};
