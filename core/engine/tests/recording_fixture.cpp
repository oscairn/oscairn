// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A CLAP plugin that shows a host what it was given (clap-host-rust compares two hosts with it): every block's steady
// time, frame count, transport, input events (bytes; a cookie as whether it is the one the plugin gave) and input audio
// go into a running hash, written into its first output channel; its second carries the input at half; the events it
// gets come back out as its output events, at their times, and a parameter value it gets comes back as a parameter
// output. At init it asks the host's extensions everything they answer (thread check, event spaces, note dialects,
// a timer, a callback) and keeps the answers' hash as its read-only parameter 3. It sleeps when a block brings nothing.
// Note input 0 takes CLAP notes, note input 1 MIDI only, so the host translates both ways; its time parameter's texts
// ("12.5 ms", "1.2 s") are what the host reads seconds from (oscairn.ui draft 4, without seconds()).
// It also has native sounds (oscairn.sound-contexts draft 5 with their own state, sound-snapshots draft 4 with a
// ring-out buffer), effect contexts (two), presets inside it ("first" loads, anything else is refused without a word),
// and a state that refuses bytes not its own, logging why; a sound's name may fill its 64 bytes without a NUL.
#include <clap/clap.h>
#include <oscairn-clap/beat.h>
#include <oscairn-clap/harmony.h>
#include <oscairn-clap/ui.h>
#include <oscairn-clap/hardware.h>
#include <oscairn-clap/sound-contexts.h>
#include <oscairn-clap/effect-contexts.h>
#include <oscairn-clap/memory.h>
#include <oscairn-clap/voice-budget.h>
#include <oscairn-clap/assets.h>
#include <cstdio>
#include <cstdlib>
#include <initializer_list>
#include <cstring>

namespace
{
    struct Recorder
    {
        clap_plugin_t plugin{};
        const clap_host_t* host = nullptr;
        uint64_t hash = 0xcbf29ce484222325ull, init = 0xcbf29ce484222325ull;
        double values[3] = {0.25, 2, 100};
        clap_id timer = CLAP_INVALID_ID;
        uint16_t beatSpace = 0, harmonySpace = 0;
        struct Sound { uint32_t id; char name[64]; } sounds[4]{{1, "Default"}};
        uint32_t soundCount = 1, nextSound = 2;
        bool ringOut = false;
    };
    const clap_plugin_t* create(const clap_plugin_factory_t*, const clap_host_t* host, const char* id);
    int effectContexts = 0;
    void fold(uint64_t& h, const void* data, size_t size) { const auto* p = static_cast<const uint8_t*>(data); for(size_t i = 0; i < size; ++i) h = (h ^ p[i]) * 0x100000001b3ull; }
    template<class T> void fold(uint64_t& h, const T& v) { fold(h, &v, sizeof v); }
    Recorder& self(const clap_plugin_t* p) { return *static_cast<Recorder*>(p->plugin_data); }

    const clap_plugin_descriptor_t descriptor{CLAP_VERSION, "org.oscairn.test.recorder", "Recorder", "Oscairn", "", "", "", "0.1", "Shows a host what it was given", nullptr};

    // Parameters: 0 a level, 1 a stepped mode, 2 a time in milliseconds (an envelope's attack), 3 the init hash (read-only).
    uint32_t paramCount(const clap_plugin_t*) { return 4; }
    bool paramInfo(const clap_plugin_t* p, uint32_t i, clap_param_info_t* info)
    {
        if(i > 3) return false;
        *info = {};
        info->id = i; info->cookie = &self(p).values[i < 3 ? i : 0];
        const char* names[] = {"Level", "Mode", "Attack", "Init"};
        std::snprintf(info->name, sizeof info->name, "%s", names[i]);
        std::snprintf(info->module, sizeof info->module, "Recorder/%u", i);
        info->min_value = 0; info->max_value = i == 1 ? 4 : i == 2 ? 2000 : i == 3 ? 16777216 : 1;
        info->default_value = i == 0 ? 0.25 : i == 1 ? 2 : i == 2 ? 100 : 0;
        info->flags = (i == 1 ? CLAP_PARAM_IS_STEPPED : 0) | (i == 3 ? CLAP_PARAM_IS_READONLY : CLAP_PARAM_IS_AUTOMATABLE);
        return true;
    }
    bool paramValue(const clap_plugin_t* p, clap_id id, double* v) { if(id > 3) return false; *v = id == 3 ? double(self(p).init & 0xffffff) : self(p).values[id]; return true; }
    bool valueText(const clap_plugin_t*, clap_id id, double v, char* text, uint32_t size)
    {
        if(id == 2) std::snprintf(text, size, v < 1000 ? "%g ms" : "%g s", v < 1000 ? v : v / 1000);
        else std::snprintf(text, size, "%.3f", v);
        return true;
    }
    bool textValue(const clap_plugin_t*, clap_id, const char* text, double* v) { *v = std::atof(text); return true; }
    void take(Recorder& r, const clap_input_events_t* in, const clap_output_events_t* out, bool echo)
    {
        for(uint32_t i = 0; i < in->size(in); ++i)
        {
            const auto* h = in->get(in, i);
            uint8_t bytes[64]{}; std::memcpy(bytes, h, h->size < 64 ? h->size : 64);
            if(h->space_id == CLAP_CORE_EVENT_SPACE_ID && h->type == CLAP_EVENT_PARAM_VALUE)
            {
                auto e = *reinterpret_cast<const clap_event_param_value_t*>(h);
                const bool cookie = e.param_id < 3 && e.cookie == &r.values[e.param_id];
                e.cookie = nullptr; std::memcpy(bytes, &e, sizeof e); fold(r.hash, cookie);
                if(e.param_id < 3) { r.values[e.param_id] = e.value; out->try_push(out, &e.header); } // reported back (without its cookie, an address): a parameter output
            }
            fold(r.hash, bytes, sizeof bytes);
            if(echo && h->space_id == CLAP_CORE_EVENT_SPACE_ID && (h->type == CLAP_EVENT_NOTE_ON || h->type == CLAP_EVENT_NOTE_OFF || h->type == CLAP_EVENT_MIDI))
                out->try_push(out, h);
        }
    }
    void flush(const clap_plugin_t* p, const clap_input_events_t* in, const clap_output_events_t* out) { take(self(p), in, out, false); }
    const clap_plugin_params_t params{paramCount, paramInfo, paramValue, valueText, textValue, flush};

    uint32_t audioCount(const clap_plugin_t*, bool) { return 1; }
    bool audioInfo(const clap_plugin_t*, uint32_t i, bool input, clap_audio_port_info_t* info)
    {
        if(i) return false;
        *info = {}; info->id = input ? 10 : 20; std::snprintf(info->name, sizeof info->name, input ? "In" : "Out");
        info->channel_count = 2; info->flags = CLAP_AUDIO_PORT_IS_MAIN; info->port_type = CLAP_PORT_STEREO; info->in_place_pair = CLAP_INVALID_ID;
        return true;
    }
    const clap_plugin_audio_ports_t audio{audioCount, audioInfo};
    uint32_t noteCount(const clap_plugin_t*, bool input) { return input ? 2 : 1; }
    bool noteInfo(const clap_plugin_t*, uint32_t i, bool input, clap_note_port_info_t* info)
    {
        if(i >= (input ? 2u : 1u)) return false;
        *info = {}; info->id = input ? 30 + i : 40;
        info->supported_dialects = input ? (i ? CLAP_NOTE_DIALECT_MIDI : CLAP_NOTE_DIALECT_CLAP) : CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI;
        info->preferred_dialect = input && i ? CLAP_NOTE_DIALECT_MIDI : CLAP_NOTE_DIALECT_CLAP;
        std::snprintf(info->name, sizeof info->name, "Notes %u", i);
        return true;
    }
    const clap_plugin_note_ports_t notes{noteCount, noteInfo};
    bool hint(const clap_plugin_t*, clap_id id, oscairn_param_hint_t* h)
    {
        if(id == 0) { *h = {OSCAIRN_WIDGET_KNOB, 1, CLAP_INVALID_ID, 0, OSCAIRN_ROLE_NONE, 0, OSCAIRN_FIELD_NONE, 0}; return true; }
        if(id == 2) { *h = {OSCAIRN_WIDGET_SLIDER, 2, CLAP_INVALID_ID, 1, OSCAIRN_ROLE_NONE, 1, OSCAIRN_FIELD_ATTACK, 0}; return true; }
        return false;
    }
    const oscairn_plugin_ui_t ui{hint, nullptr, nullptr}; // offered as draft 4: no seconds(), so the host reads the texts
    void onTimer(const clap_plugin_t* p, clap_id id) { fold(self(p).init, id); }
    const clap_plugin_timer_support_t timers{onTimer};

    // Its state: "REC1", the three values and the hash; anything else refused, the reason logged.
    bool saveState(const clap_plugin_t* p, const clap_ostream_t* out)
    {
        auto& r = self(p); uint8_t b[36]; std::memcpy(b, "REC1", 4); std::memcpy(b + 4, r.values, 24); std::memcpy(b + 28, &r.hash, 8);
        for(size_t done = 0; done < sizeof b;) { const auto n = out->write(out, b + done, done ? sizeof b - done : 5); if(n <= 0) return false; done += size_t(n); } // in two writes
        return true;
    }
    bool loadState(const clap_plugin_t* p, const clap_istream_t* in)
    {
        auto& r = self(p); uint8_t b[37]{}; int64_t got = 0;
        for(int64_t n; (n = in->read(in, b + got, sizeof b - got)) > 0;) got += n;
        if(got != 36 || std::memcmp(b, "REC1", 4))
        {
            if(const auto* l = static_cast<const clap_host_log_t*>(r.host->get_extension(r.host, CLAP_EXT_LOG))) l->log(r.host, CLAP_LOG_ERROR, "recorder: not its state");
            return false;
        }
        std::memcpy(r.values, b + 4, 24); fold(r.hash, b + 28, 8);
        return true;
    }
    const clap_plugin_state_t state{saveState, loadState};
    uint32_t capacity(const clap_plugin_t*) { return 4; }
    bool getSound(const clap_plugin_t* p, uint32_t slot, oscairn_sound_context_info_t* info)
    {
        auto& r = self(p); if(slot >= r.soundCount) return false;
        info->id = r.sounds[slot].id; std::memcpy(info->name, r.sounds[slot].name, 64); return true;
    }
    uint32_t addSound(Recorder& r, const char* name)
    {
        if(r.soundCount == 4) return CLAP_INVALID_ID;
        auto& x = r.sounds[r.soundCount++]; x.id = r.nextSound++; std::memset(x.name, 'x', 64); std::memcpy(x.name, name, std::strlen(name) < 64 ? std::strlen(name) + (std::strlen(name) < 8) : 64);
        return x.id;
    }
    uint32_t prepare(const clap_plugin_t* p, const char* name, const oscairn_sound_parameter_t* values, uint32_t count, const char* patch, const oscairn_asset_ref_t* base, char* message, uint32_t size)
    {
        auto& r = self(p);
        for(uint32_t i = 0; i < count; ++i) fold(r.hash, values[i]);
        fold(r.hash, patch != nullptr);
        // A patch's files through the host's asset store (oscairn.assets draft 4), every answer kept.
        if(const auto* a = static_cast<const oscairn_host_assets_t*>(r.host->get_extension(r.host, OSCAIRN_EXT_ASSETS)); a && patch)
        {
            oscairn_asset_ref_t ref{}; std::snprintf(ref.library, sizeof ref.library, "%s", base ? base->library : "none"); std::snprintf(ref.path, sizeof ref.path, "%s/%s", base ? base->path : "", patch);
            char path[256]{}, text[128]{};
            fold(r.hash, a->resolve(r.host, &ref, path, sizeof path, text, sizeof text)); fold(r.hash, path, sizeof path); fold(r.hash, text, sizeof text); fold(r.hash, ref.sha256, sizeof ref.sha256);
            fold(r.hash, a->resolve(r.host, &ref, path, 4, text, sizeof text)); fold(r.hash, text, sizeof text); // too small for it
            std::memset(text, 0, sizeof text); std::memset(path, 0, sizeof path);
            fold(r.hash, a->library_root(r.host, ref.library, "1.0", path, sizeof path)); fold(r.hash, path, sizeof path);
            fold(r.hash, a->create(r.host, "/tmp/made.wav", &ref, text, sizeof text)); fold(r.hash, text, sizeof text);
            std::snprintf(ref.path, sizeof ref.path, "sample:%064d", 7); oscairn_sample_location_t at{};
            fold(r.hash, a->resolve_sample(r.host, &ref, path, sizeof path, &at, text, sizeof text)); fold(r.hash, path, sizeof path); fold(r.hash, at);
            std::memset(ref.sha256, 0, sizeof ref.sha256); fold(r.hash, a->resolve_sample(r.host, &ref, path, sizeof path, &at, text, sizeof text)); fold(r.hash, path, sizeof path); fold(r.hash, at);
            std::snprintf(ref.path, sizeof ref.path, "not-a-sample"); fold(r.hash, a->resolve_sample(r.host, &ref, path, sizeof path, &at, text, sizeof text)); fold(r.hash, text, sizeof text);
        }
        if(!std::strcmp(name, "Reject")) { std::snprintf(message, size, "rejected by the recorder"); return CLAP_INVALID_ID; }
        return addSound(r, name);
    }
    bool select(const clap_plugin_t* p, uint32_t id, int16_t channel)
    {
        auto& r = self(p); fold(r.hash, id); fold(r.hash, channel);
        for(uint32_t i = 0; i < r.soundCount; ++i) if(r.sounds[i].id == id) return true;
        return false;
    }
    bool discard(const clap_plugin_t* p, uint32_t id)
    {
        auto& r = self(p);
        for(uint32_t i = 1; i < r.soundCount; ++i) if(r.sounds[i].id == id) { r.sounds[i] = r.sounds[--r.soundCount]; return true; }
        return false;
    }
    uint32_t preparePreset(const clap_plugin_t* p, const char* name, uint32_t kind, const char* location, const char* key, char* message, uint32_t size)
    {
        auto& r = self(p); fold(r.hash, kind); fold(r.hash, location != nullptr);
        if(!key || std::strcmp(key, "first")) { std::snprintf(message, size, "no such preset"); return CLAP_INVALID_ID; }
        return addSound(r, name);
    }
    const oscairn_plugin_sound_contexts_t soundContexts{capacity, getSound, prepare, select, discard, &state, preparePreset};
    bool prepareSnapshot(const clap_plugin_t* p, const clap_istream_t* in, uint32_t* selected, char* message, uint32_t size)
    {
        auto& r = self(p); uint8_t b[64]{}; int64_t got = 0;
        for(int64_t n; (n = in->read(in, b + got, sizeof b - got)) > 0;) got += n;
        if(got != 36 || std::memcmp(b, "REC1", 4)) { std::snprintf(message, size, "not a recorder snapshot"); return false; }
        selected[0] = addSound(r, "Snapshot"); selected[3] = r.sounds[0].id;
        return selected[0] != CLAP_INVALID_ID;
    }
    void ringOut(const clap_plugin_t* p, const clap_audio_buffer_t* buffer) { auto& r = self(p); r.ringOut = buffer != nullptr; if(buffer) fold(r.hash, buffer->channel_count); }
    bool choke(const clap_plugin_t* p) { fold(self(p).hash, uint32_t(0xc40e)); return true; }
    bool saveSound(const clap_plugin_t* p, uint32_t id, const clap_ostream_t* out) { fold(self(p).hash, id); return select(p, id, -1) && saveState(p, out); }
    uint32_t soundValues(const clap_plugin_t* p, uint32_t id, oscairn_sound_parameter_t* values, uint32_t size)
    {
        if(!select(p, id, -2) || size < 2) return 0;
        values[0] = {0, 0.5}; values[1] = {2, double(id)}; return 2;
    }
    const oscairn_plugin_sound_snapshots_t snapshots{prepareSnapshot, ringOut, choke, saveSound, soundValues};
    const clap_plugin_t* createContext(const clap_plugin_t*, const clap_host_t* host) { return effectContexts++ >= 2 ? nullptr : create(nullptr, host, descriptor.id); }
    bool resources(const clap_plugin_t*, oscairn_effect_resources_t* r) { *r = {1 << 20, 4096, 2, uint32_t(1 + effectContexts), false}; return true; }
    const oscairn_plugin_effect_contexts_t effects{createContext, resources};
    bool fromLocation(const clap_plugin_t* p, uint32_t kind, const char* location, const char* key)
    {
        fold(self(p).hash, kind); fold(self(p).hash, location != nullptr);
        if(key && !std::strcmp(key, "first")) { self(p).values[0] = 0.75; return true; }
        return false;
    }
    const clap_plugin_preset_load_t presets{fromLocation};
    bool estimate(const clap_plugin_t*, double rate, uint32_t frames, oscairn_memory_t* m) { *m = {uint64_t(rate) * 8, uint64_t(frames) * 16, 1024}; return true; }
    bool current(const clap_plugin_t*, oscairn_memory_t* m) { *m = {1, 2, 3}; return true; }
    const oscairn_plugin_memory_t memory{estimate, current};
    // oscairn.voice-budget: the limits it is given go into its hash; its report is the notes it was last given.
    bool honorsVolume(const clap_plugin_t*) { return true; }
    void setLimits(const clap_plugin_t* p, uint32_t limit, const oscairn_voice_budget_context_t* contexts, uint32_t count)
    {
        auto& r = self(p); fold(r.hash, limit); fold(r.hash, count);
        for(uint32_t i = 0; i < count; ++i) { const auto& c = contexts[i]; fold(r.hash, c.context); fold(r.hash, c.limit); fold(r.hash, c.reserve); fold(r.hash, c.priority); fold(r.hash, c.flags); }
    }
    uint32_t activeVoices(const clap_plugin_t* p, double* cost) { *cost = double(self(p).hash % 1000) / 7; return uint32_t(self(p).hash % 5); }
    uint32_t voiceNotes(const clap_plugin_t* p, oscairn_voice_budget_note_t* notes, uint32_t capacity)
    {
        const auto n = uint32_t(self(p).hash % 4) < capacity ? uint32_t(self(p).hash % 4) : capacity;
        for(uint32_t i = 0; i < n; ++i) notes[i] = {int32_t(i), 0, int16_t(i), int16_t(60 + i), i + 1, 0.25f * float(i)};
        return n;
    }
    const oscairn_plugin_voice_budget_t voiceBudget{honorsVolume, setLimits, activeVoices, voiceNotes};

    const void* extension(const clap_plugin_t*, const char* id)
    {
        if(!std::strcmp(id, OSCAIRN_EXT_SOUND_CONTEXTS)) return &soundContexts;
        if(!std::strcmp(id, OSCAIRN_EXT_SOUND_SNAPSHOTS)) return &snapshots;
        if(!std::strcmp(id, OSCAIRN_EXT_EFFECT_CONTEXTS)) return &effects;
        if(!std::strcmp(id, CLAP_EXT_PRESET_LOAD)) return &presets;
        if(!std::strcmp(id, OSCAIRN_EXT_MEMORY)) return &memory;
        if(!std::strcmp(id, OSCAIRN_EXT_VOICE_BUDGET)) return &voiceBudget;
        if(!std::strcmp(id, CLAP_EXT_PARAMS)) return &params;
        if(!std::strcmp(id, CLAP_EXT_AUDIO_PORTS)) return &audio;
        if(!std::strcmp(id, CLAP_EXT_NOTE_PORTS)) return &notes;
        if(!std::strcmp(id, OSCAIRN_EXT_UI_DRAFT_4)) return &ui;
        if(!std::strcmp(id, CLAP_EXT_TIMER_SUPPORT)) return &timers;
        return nullptr;
    }
    bool init(const clap_plugin_t* p)
    {
        auto& r = self(p); const auto* h = r.host;
        fold(r.init, h->clap_version); for(const char* s : {h->name, h->vendor, h->url, h->version}) fold(r.init, s, std::strlen(s));
        if(const auto* t = static_cast<const clap_host_thread_check_t*>(h->get_extension(h, CLAP_EXT_THREAD_CHECK))) { fold(r.init, t->is_main_thread(h)); fold(r.init, t->is_audio_thread(h)); }
        if(const auto* e = static_cast<const clap_host_event_registry_t*>(h->get_extension(h, CLAP_EXT_EVENT_REGISTRY)))
        {
            uint16_t space = 0;
            fold(r.init, e->query(h, OSCAIRN_EVENT_SPACE_BEAT, &space)); r.beatSpace = space; fold(r.init, space);
            fold(r.init, e->query(h, OSCAIRN_EVENT_SPACE_HARMONY, &space)); r.harmonySpace = space; fold(r.init, space);
            fold(r.init, e->query(h, "no.such.space", &space));
        }
        if(const auto* n = static_cast<const clap_host_note_ports_t*>(h->get_extension(h, CLAP_EXT_NOTE_PORTS))) fold(r.init, n->supported_dialects(h));
        if(const auto* a = static_cast<const clap_host_audio_ports_t*>(h->get_extension(h, CLAP_EXT_AUDIO_PORTS))) for(uint32_t f = 1; f; f <<= 1) fold(r.init, a->is_rescan_flag_supported(h, f));
        if(const auto* t = static_cast<const clap_host_timer_support_t*>(h->get_extension(h, CLAP_EXT_TIMER_SUPPORT))) { fold(r.init, t->register_timer(h, 1, &r.timer)); fold(r.init, r.timer); }
        for(const char* id : {CLAP_EXT_GUI, CLAP_EXT_LOG, CLAP_EXT_STATE, CLAP_EXT_LATENCY, CLAP_EXT_TAIL, CLAP_EXT_PARAMS, CLAP_EXT_POSIX_FD_SUPPORT, CLAP_EXT_PRESET_LOAD,
                              OSCAIRN_EXT_HARMONY, OSCAIRN_EXT_HARDWARE, "oscairn.assets/draft-4", "no.such.extension"}) fold(r.init, h->get_extension(h, id) != nullptr);
        if(const auto* l = static_cast<const clap_host_log_t*>(h->get_extension(h, CLAP_EXT_LOG))) l->log(h, CLAP_LOG_INFO, "recorder ready");
        h->request_callback(h);
        return true;
    }
    void onMainThread(const clap_plugin_t* p) { fold(self(p).init, uint32_t(0xc0ffee)); }
    clap_process_status process(const clap_plugin_t* p, const clap_process_t* b)
    {
        auto& r = self(p);
        fold(r.hash, b->steady_time); fold(r.hash, b->frames_count);
        fold(r.hash, b->transport != nullptr); if(b->transport) fold(r.hash, *b->transport);
        fold(r.hash, b->audio_inputs_count); fold(r.hash, b->audio_outputs_count);
        bool sound = false;
        for(uint32_t c = 0; c < b->audio_inputs[0].channel_count; ++c) { fold(r.hash, b->audio_inputs[0].data32[c], b->frames_count * sizeof(float)); for(uint32_t i = 0; i < b->frames_count; ++i) sound = sound || b->audio_inputs[0].data32[c][i] != 0; }
        bool events = false; // the beat and harmony alone are no reason to wake (it sleeps through them, as a host may let it)
        for(uint32_t i = 0; i < b->in_events->size(b->in_events); ++i) events = events || b->in_events->get(b->in_events, i)->space_id == CLAP_CORE_EVENT_SPACE_ID;
        take(r, b->in_events, b->out_events, true);
        for(uint32_t i = 0; i < b->frames_count; ++i)
        {
            b->audio_outputs[0].data32[0][i] = float((r.hash >> ((i % 8) * 8)) & 0xff) / 256.f;
            b->audio_outputs[0].data32[1][i] = b->audio_inputs[0].data32[0][i] * 0.5f;
        }
        return events || sound ? CLAP_PROCESS_CONTINUE : CLAP_PROCESS_SLEEP;
    }
    const clap_plugin_t* create(const clap_plugin_factory_t*, const clap_host_t* host, const char* id)
    {
        if(std::strcmp(id, descriptor.id)) return nullptr;
        auto* r = new Recorder; r->host = host;
        r->plugin = {&descriptor, r, init, [](const clap_plugin_t* p) { delete &self(p); },
                     [](const clap_plugin_t*, double, uint32_t, uint32_t) { return true; }, [](const clap_plugin_t*) {},
                     [](const clap_plugin_t*) { return true; }, [](const clap_plugin_t*) {}, [](const clap_plugin_t* p) { fold(self(p).hash, uint32_t(0x5e7)); },
                     process, extension, onMainThread};
        return &r->plugin;
    }
    const clap_plugin_factory_t factory{[](const clap_plugin_factory_t*) { return 1u; }, [](const clap_plugin_factory_t*, uint32_t i) { return i ? nullptr : &descriptor; }, create};
}

extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{CLAP_VERSION, [](const char*) { return true; }, [] {},
                                                             [](const char* id) -> const void* { return std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) ? nullptr : &factory; }};
