// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A silent instrument for the desktop's tests that describes every rich view oscairn.ui draft 4
// has, as the engines will: an FM operator view (Common: Algorithm 1-32 with DX7's routings and
// Feedback; Operator 1-6: Level, Coarse, Fine, a DX7 envelope of rates and levels, and keyboard
// level scaling), and two AWM2-style elements (Element 1-2: key and velocity limits in one map
// across both, velocity depth, and an envelope of times and levels from a hold).
#include <clap/clap.h>
#include <oscairn-clap/ui.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace
{
    struct Param { std::string module, name; double maximum, initial; uint32_t view, field, index; bool stepped = true; };

    const char* const curves[] = {"-LIN", "-EXP", "+EXP", "+LIN"};

    std::vector<Param> table()
    {
        std::vector<Param> t;
        t.push_back({"Common", "Algorithm", 32, 1, 1, OSCAIRN_FIELD_ALGORITHM, 0});
        t.back().initial = 5;
        t.push_back({"Common", "Feedback", 7, 0, 1, OSCAIRN_FIELD_FEEDBACK, 0});
        for(uint32_t n = 1; n <= 6; ++n)
        {
            const auto m = "Operator " + std::to_string(n);
            t.push_back({m, "Level", 99, n == 1 ? 99.0 : 70.0, 1, OSCAIRN_FIELD_OPERATOR_LEVEL, n});
            t.push_back({m, "Coarse", 31, 1, 1, OSCAIRN_FIELD_RATIO, n});
            t.push_back({m, "Fine", 99, 0, 1, OSCAIRN_FIELD_FINE, n});
            for(uint32_t k = 0; k < 4; ++k) t.push_back({m, "Rate " + std::to_string(k + 1), 99, k == 3 ? 60.0 : 80.0 - 20 * k, 1, OSCAIRN_FIELD_RATE_1 + k, n});
            for(uint32_t k = 0; k < 4; ++k) t.push_back({m, "Level " + std::to_string(k + 1), 99, k == 0 ? 99.0 : k == 1 ? 80.0 : k == 2 ? 60.0 : 0.0, 1, OSCAIRN_FIELD_LEVEL_0 + k + 1, n});
            t.push_back({m, "Break Point", 99, 39, 1, OSCAIRN_FIELD_BREAKPOINT, n});
            t.push_back({m, "Left Depth", 99, 30, 1, OSCAIRN_FIELD_LEFT_DEPTH, n});
            t.push_back({m, "Right Depth", 99, 50, 1, OSCAIRN_FIELD_RIGHT_DEPTH, n});
            t.push_back({m, "Left Curve", 3, 0, 1, OSCAIRN_FIELD_LEFT_CURVE, n});
            t.push_back({m, "Right Curve", 3, 1, 1, OSCAIRN_FIELD_RIGHT_CURVE, n});
        }
        for(uint32_t n = 1; n <= 2; ++n)
        {
            const auto m = "Element " + std::to_string(n);
            t.push_back({m, "Note Low", 127, n == 1 ? 0.0 : 60.0, 2, OSCAIRN_FIELD_KEY_LOW, n});
            t.push_back({m, "Note High", 127, n == 1 ? 71.0 : 127.0, 2, OSCAIRN_FIELD_KEY_HIGH, n});
            t.push_back({m, "Velocity Low", 127, n == 1 ? 1.0 : 64.0, 2, OSCAIRN_FIELD_VELOCITY_LOW, n});
            t.push_back({m, "Velocity High", 127, 127, 2, OSCAIRN_FIELD_VELOCITY_HIGH, n});
            t.push_back({m, "Velocity Sensitivity", 127, 64, 2, OSCAIRN_FIELD_VELOCITY_DEPTH, n});
            const uint32_t eg = 2 + n; // each element's envelope is a view of its own
            t.push_back({m, "Hold Time", 127, 0, eg, OSCAIRN_FIELD_TIME_1, 0});
            t.push_back({m, "Attack Time", 127, 10, eg, OSCAIRN_FIELD_TIME_1 + 1, 0});
            t.push_back({m, "Decay 1 Time", 127, 40, eg, OSCAIRN_FIELD_TIME_1 + 2, 0});
            t.push_back({m, "Decay 2 Time", 127, 60, eg, OSCAIRN_FIELD_TIME_1 + 3, 0});
            t.push_back({m, "Release Time", 127, 50, eg, OSCAIRN_FIELD_TIME_1 + 4, 0});
            t.push_back({m, "Initial Level", 127, 0, eg, OSCAIRN_FIELD_LEVEL_0, 0});
            t.push_back({m, "Attack Level", 127, 127, eg, OSCAIRN_FIELD_LEVEL_0 + 2, 0});
            t.push_back({m, "Decay 1 Level", 127, 100, eg, OSCAIRN_FIELD_LEVEL_0 + 3, 0});
            t.push_back({m, "Decay 2 Level", 127, 80, eg, OSCAIRN_FIELD_LEVEL_0 + 4, 0});
        }
        return t;
    }
    const std::vector<Param>& params() { static const auto t = table(); return t; }

    // DX7's 32 algorithms: per operator the operators modulating it, the carriers, and feedback
    // (from, to), operators numbered from 1 as on its panel.
    constexpr uint8_t op(int n) { return uint8_t(1u << (n - 1)); }
    const oscairn_fm_algorithm_t dx7[32] = {
        {{op(2), 0, op(4), op(5), op(6), 0}, op(1) | op(3), 6, 6},
        {{op(2), 0, op(4), op(5), op(6), 0}, op(1) | op(3), 2, 2},
        {{op(2), op(3), 0, op(5), op(6), 0}, op(1) | op(4), 6, 6},
        {{op(2), op(3), 0, op(5), op(6), 0}, op(1) | op(4), 4, 6},
        {{op(2), 0, op(4), 0, op(6), 0}, op(1) | op(3) | op(5), 6, 6},
        {{op(2), 0, op(4), 0, op(6), 0}, op(1) | op(3) | op(5), 5, 6},
        {{op(2), 0, op(4) | op(5), 0, op(6), 0}, op(1) | op(3), 6, 6},
        {{op(2), 0, op(4) | op(5), 0, op(6), 0}, op(1) | op(3), 4, 4},
        {{op(2), 0, op(4) | op(5), 0, op(6), 0}, op(1) | op(3), 2, 2},
        {{op(2), op(3), 0, op(5) | op(6), 0, 0}, op(1) | op(4), 3, 3},
        {{op(2), op(3), 0, op(5) | op(6), 0, 0}, op(1) | op(4), 6, 6},
        {{op(2), 0, op(4) | op(5) | op(6), 0, 0, 0}, op(1) | op(3), 2, 2},
        {{op(2), 0, op(4) | op(5) | op(6), 0, 0, 0}, op(1) | op(3), 6, 6},
        {{op(2), 0, op(4), op(5) | op(6), 0, 0}, op(1) | op(3), 6, 6},
        {{op(2), 0, op(4), op(5) | op(6), 0, 0}, op(1) | op(3), 2, 2},
        {{op(2) | op(3) | op(5), 0, op(4), 0, op(6), 0}, op(1), 6, 6},
        {{op(2) | op(3) | op(5), 0, op(4), 0, op(6), 0}, op(1), 2, 2},
        {{op(2) | op(3) | op(4), 0, 0, op(5), op(6), 0}, op(1), 3, 3},
        {{op(2), op(3), 0, op(6), op(6), 0}, op(1) | op(4) | op(5), 6, 6},
        {{op(3), op(3), 0, op(5) | op(6), 0, 0}, op(1) | op(2) | op(4), 3, 3},
        {{op(3), op(3), 0, op(6), op(6), 0}, op(1) | op(2) | op(4) | op(5), 3, 3},
        {{op(2), 0, op(6), op(6), op(6), 0}, op(1) | op(3) | op(4) | op(5), 6, 6},
        {{0, op(3), 0, op(6), op(6), 0}, op(1) | op(2) | op(4) | op(5), 6, 6},
        {{0, 0, op(6), op(6), op(6), 0}, op(1) | op(2) | op(3) | op(4) | op(5), 6, 6},
        {{0, 0, 0, op(6), op(6), 0}, op(1) | op(2) | op(3) | op(4) | op(5), 6, 6},
        {{0, op(3), 0, op(5) | op(6), 0, 0}, op(1) | op(2) | op(4), 6, 6},
        {{0, op(3), 0, op(5) | op(6), 0, 0}, op(1) | op(2) | op(4), 3, 3},
        {{op(2), 0, op(4), op(5), 0, 0}, op(1) | op(3) | op(6), 5, 5},
        {{0, 0, op(4), 0, op(6), 0}, op(1) | op(2) | op(3) | op(5), 6, 6},
        {{0, 0, op(4), op(5), 0, 0}, op(1) | op(2) | op(3) | op(6), 5, 5},
        {{0, 0, 0, 0, op(6), 0}, op(1) | op(2) | op(3) | op(4) | op(5), 6, 6},
        {{0, 0, 0, 0, 0, 0}, op(1) | op(2) | op(3) | op(4) | op(5) | op(6), 6, 6},
    };

    std::string note(int n) { static const char* const names[] = {"C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"}; return names[((n % 12) + 12) % 12] + std::to_string(n / 12 - 2); } // Yamaha's: 60 is C3

    struct Plugin
    {
        clap_plugin_t plugin{};
        std::vector<std::atomic<double>> values = std::vector<std::atomic<double>>(params().size());
        Plugin() { for(size_t i = 0; i < values.size(); ++i) values[i] = params()[i].initial; }
    };
    Plugin& self(const clap_plugin_t* p) { return *static_cast<Plugin*>(p->plugin_data); }

    const clap_plugin_params_t paramsExt{
        [](const clap_plugin_t*) { return uint32_t(params().size()); },
        [](const clap_plugin_t*, uint32_t i, clap_param_info_t* out) {
            if(i >= params().size()) return false;
            const auto& p = params()[i];
            *out = {}; out->id = i + 1; out->flags = CLAP_PARAM_IS_AUTOMATABLE | (p.stepped ? CLAP_PARAM_IS_STEPPED : 0);
            out->min_value = p.field == OSCAIRN_FIELD_ALGORITHM ? 1 : 0; out->max_value = p.maximum; out->default_value = p.initial;
            std::snprintf(out->name, sizeof out->name, "%s", p.name.c_str()); std::snprintf(out->module, sizeof out->module, "%s", p.module.c_str());
            return true;
        },
        [](const clap_plugin_t* pl, clap_id id, double* v) { if(id < 1 || id > params().size()) return false; *v = self(pl).values[id - 1]; return true; },
        [](const clap_plugin_t*, clap_id id, double v, char* out, uint32_t size) {
            if(id < 1 || id > params().size()) return false;
            const auto& p = params()[id - 1];
            const int n = int(std::lround(v));
            if(p.field == OSCAIRN_FIELD_LEFT_CURVE || p.field == OSCAIRN_FIELD_RIGHT_CURVE) std::snprintf(out, size, "%s", curves[std::clamp(n, 0, 3)]);
            else if(p.field == OSCAIRN_FIELD_BREAKPOINT) std::snprintf(out, size, "%s", note(n + 21).c_str()); // DX7's: 0 is A-1
            else if(p.field == OSCAIRN_FIELD_KEY_LOW || p.field == OSCAIRN_FIELD_KEY_HIGH) std::snprintf(out, size, "%s", note(n).c_str());
            else if(p.field == OSCAIRN_FIELD_RATIO) std::snprintf(out, size, "x%.2f", n == 0 ? .5 : double(n));
            else std::snprintf(out, size, "%d", n);
            return true;
        },
        [](const clap_plugin_t*, clap_id, const char* text, double* v) { *v = std::atof(text); return true; },
        [](const clap_plugin_t* pl, const clap_input_events_t* in, const clap_output_events_t*) {
            for(uint32_t i = 0; i < in->size(in); ++i)
                if(const auto* e = in->get(in, i); e->space_id == CLAP_CORE_EVENT_SPACE_ID && e->type == CLAP_EVENT_PARAM_VALUE)
                {
                    const auto* v = reinterpret_cast<const clap_event_param_value_t*>(e);
                    if(v->param_id >= 1 && v->param_id <= params().size()) self(pl).values[v->param_id - 1] = v->value;
                }
        }};
    const oscairn_plugin_ui_t ui{
        [](const clap_plugin_t*, clap_id id, oscairn_param_hint_t* hint) {
            if(id < 1 || id > params().size()) return false;
            const auto& p = params()[id - 1];
            *hint = {OSCAIRN_WIDGET_AUTO, 0, CLAP_INVALID_ID, 0, OSCAIRN_ROLE_NONE, p.view, p.field, p.index};
            return true;
        },
        [](const clap_plugin_t*, clap_id id, uint32_t value, oscairn_fm_algorithm_t* out) {
            if(id != 1 || value < 1 || value > 32) return false;
            *out = dx7[value - 1];
            return true;
        }};
    const clap_plugin_audio_ports_t audio{
        [](const clap_plugin_t*, bool input) -> uint32_t { return input ? 0 : 1; },
        [](const clap_plugin_t*, uint32_t i, bool input, clap_audio_port_info_t* out) {
            if(i || input) return false;
            *out = {}; out->id = 0; std::strcpy(out->name, "Main"); out->flags = CLAP_AUDIO_PORT_IS_MAIN; out->channel_count = 2; out->port_type = CLAP_PORT_STEREO; out->in_place_pair = CLAP_INVALID_ID;
            return true;
        }};
    const clap_plugin_note_ports_t notes{
        [](const clap_plugin_t*, bool input) -> uint32_t { return input ? 1 : 0; },
        [](const clap_plugin_t*, uint32_t i, bool input, clap_note_port_info_t* out) {
            if(i || !input) return false;
            *out = {}; out->supported_dialects = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI; out->preferred_dialect = CLAP_NOTE_DIALECT_CLAP; std::strcpy(out->name, "Notes");
            return true;
        }};

    const char* const features[] = {CLAP_PLUGIN_FEATURE_INSTRUMENT, nullptr};
    const clap_plugin_descriptor_t descriptor{CLAP_VERSION_INIT, "org.oscairn.test.views", "Oscairn Views Test", "Oscairn", "", "", "", "0.1.0", "Describes oscairn.ui's rich views, for tests", features};

    const clap_plugin_t* create()
    {
        auto* s = new Plugin;
        auto& p = s->plugin;
        p.desc = &descriptor; p.plugin_data = s;
        p.init = [](const clap_plugin_t*) { return true; };
        p.destroy = [](const clap_plugin_t* pl) { delete &self(pl); };
        p.activate = [](const clap_plugin_t*, double, uint32_t, uint32_t) { return true; };
        p.deactivate = [](const clap_plugin_t*) {};
        p.start_processing = [](const clap_plugin_t*) { return true; };
        p.stop_processing = [](const clap_plugin_t*) {};
        p.reset = [](const clap_plugin_t*) {};
        p.process = [](const clap_plugin_t* pl, const clap_process_t* process) -> clap_process_status {
            paramsExt.flush(pl, process->in_events, process->out_events);
            for(uint32_t c = 0; c < process->audio_outputs[0].channel_count; ++c) std::memset(process->audio_outputs[0].data32[c], 0, process->frames_count * sizeof(float)); // silent
            return CLAP_PROCESS_SLEEP;
        };
        p.get_extension = [](const clap_plugin_t*, const char* id) -> const void* {
            if(!std::strcmp(id, CLAP_EXT_PARAMS)) return &paramsExt;
            if(!std::strcmp(id, OSCAIRN_EXT_UI)) return &ui;
            if(!std::strcmp(id, CLAP_EXT_AUDIO_PORTS)) return &audio;
            if(!std::strcmp(id, CLAP_EXT_NOTE_PORTS)) return &notes;
            return nullptr;
        };
        p.on_main_thread = [](const clap_plugin_t*) {};
        return &p;
    }
    const clap_plugin_factory_t factory{
        [](const clap_plugin_factory_t*) -> uint32_t { return 1; },
        [](const clap_plugin_factory_t*, uint32_t i) -> const clap_plugin_descriptor_t* { return i ? nullptr : &descriptor; },
        [](const clap_plugin_factory_t*, const clap_host_t*, const char* id) -> const clap_plugin_t* { return std::strcmp(id, descriptor.id) ? nullptr : create(); }};
}

extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{
    CLAP_VERSION_INIT, [](const char*) { return true; }, [] {},
    [](const char* id) -> const void* { return std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) ? nullptr : &factory; }};
