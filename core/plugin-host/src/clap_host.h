/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* The CLAP host behind one C interface: one plugin instance, loaded from its library, with what ClapHost
 * (plugin_host.cpp, the PluginHost the engine and the plugin worker use) asks of it. Implementations:
 * cpp_clap_* (clap_host_cpp.cpp, around kick::clap::Host, the reference) and, from the next stage, Rust's.
 * plugin_host.cpp calls the one picked through OSCAIRN_CLAP(name). Rust's types are generated from this file
 * into core/engine/rust/src/bindings.rs (its test engine_core_h_unchanged fails when this file moves).
 *
 * Threads, as kick::clap::Host's (Host.h): control calls on one main thread; process and the calls named
 * audio thread on one audio thread; enqueue (send_event), statistics and the counters on any. Nothing an audio
 * thread calls allocates or locks.
 *
 * A call returning bool, or int (-1), can fail: false (or -1) and oscairn_clap_error says why on this thread.
 * Lists are given item by item to a callback, during the call; the strings and arrays it is handed live as long.
 *
 * C, so bindgen reads it; C++ sees kick::clap::Event, kick::clap::Statistics, std::atomic<uint32_t> and
 * kick::clap::AssetResolver in the same bytes (clap_host_cpp.cpp asserts their layout). */
#pragma once
#include <clap/events.h>
#include <oscairn-clap/effect-contexts.h>
#include <oscairn-clap/memory.h>
#include <oscairn-clap/sound-contexts.h>
#include <oscairn-clap/voice-budget.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#include <atomic>
#include <kick_clap/Host.h>
typedef kick::clap::Event oscairn_clap_event;
typedef kick::clap::Statistics oscairn_clap_statistics;
typedef std::atomic<uint32_t> oscairn_clap_wake;
typedef kick::clap::AssetResolver oscairn_clap_assets;
extern "C" {
#else
typedef struct { _Alignas(8) uint8_t bytes[64]; } oscairn_clap_event; /* kick::clap::Event */
typedef uint32_t oscairn_clap_wake;                                    /* an atomic word */
/* kick::clap::Statistics, field for field (Host.h says what each counts). */
typedef struct
{
    uint64_t blocks, droppedEvents, processErrors;
    double lastMicroseconds, maximumMicroseconds;
    uint64_t deadlineMisses, controlSilences, idleBlocks;
    uint64_t restarts;
    uint64_t lateBlocks, silentBlocks;
    double allowanceMicroseconds;
    uint64_t busyBlocks, missPanics, minorFaults, majorFaults, involuntarySwitches;
    uint64_t calledBlocks, restedBlocks;
    double cpuMicroseconds;
} oscairn_clap_statistics;
/* shortcut: kick::clap::AssetResolver, opaque, so only the C++ host can resolve assets; a C table of its four calls
 * replaces it when the Rust host takes assets (stage 4 of agent-notes/rust-graph.md's plan). */
typedef struct oscairn_clap_assets oscairn_clap_assets;
#endif

typedef struct oscairn_clap_host oscairn_clap_host; /* one plugin instance */

/* What oscairn_clap_error says a failure was, as the C++ exception ClapHost throws for it. */
enum { OSCAIRN_CLAP_RUNTIME_ERROR = 1, OSCAIRN_CLAP_INVALID_ARGUMENT = 2, OSCAIRN_CLAP_LOGIC_ERROR = 3 };

/* kick::clap::Preset: kind 1 inside the plugin, 0 a file at location. */
typedef struct { const char* name; uint32_t kind; const char* location; const char* load_key; } oscairn_clap_preset;
/* kick::clap::AssetReference. */
typedef struct { const char* library; const char* path; const char* sha256; bool optional; } oscairn_clap_asset;
/* kick::clap::Port. */
typedef struct { uint32_t id; const char* name; uint32_t channels, flags; } oscairn_clap_port;
/* kick::clap::Parameter (Host.h says what each hint means). */
typedef struct
{
    uint32_t id;
    const char* name;
    const char* module;
    double minimum, maximum, default_value;
    uint32_t flags, widget, priority, range_high, group, role, view, field, index;
    const uint8_t (*algorithms)[11];
    uint32_t algorithm_count;
    const double* seconds;
    uint32_t second_count;
} oscairn_clap_parameter;
/* kick::clap::Descriptor's own fields; its lists come one by one. */
typedef struct
{
    const char* path;
    const char* id;
    const char* name;
    const char* vendor;
    const char* version;
    const char* description;
    bool preset_discovery;
} oscairn_clap_descriptor;
/* What oscairn_clap_describe tells: the descriptor first, then its features, presets and param-target lanes,
 * the audio ports (inputs, then outputs) and the parameters, each list in order. */
typedef struct
{
    void* context;
    void (*descriptor)(void* context, const oscairn_clap_descriptor* descriptor);
    void (*feature)(void* context, const char* feature);
    void (*preset)(void* context, const oscairn_clap_preset* preset);
    void (*lane)(void* context, uint32_t number, const char* name);
    void (*port)(void* context, bool input, const oscairn_clap_port* port);
    void (*parameter)(void* context, const oscairn_clap_parameter* parameter);
} oscairn_clap_description;
typedef void (*oscairn_clap_text)(void* context, const char* text, size_t size);

#define OSCAIRN_CLAP_API(p) \
    /* Failures */ \
    const char* p##clap_error(int* kind); \
    /* Life: new does what loading a CLAP plugin in this process did (its library inspected, the plugin with id loaded, \
     * its audio layout locked, its first service run); effect_context makes one from a loaded instance; free unloads. */ \
    oscairn_clap_host* p##clap_new(const char* path, const char* id, double rate, uint32_t frames, bool offline, uint64_t memory_room); \
    oscairn_clap_host* p##clap_effect_context(oscairn_clap_host* source); \
    void p##clap_free(oscairn_clap_host* host); \
    /* Description: changes only when revision does (a rescan, a restart). */ \
    uint64_t p##clap_revision(const oscairn_clap_host* host); \
    void p##clap_describe(const oscairn_clap_host* host, const oscairn_clap_description* to); \
    uint32_t p##clap_note_input_ports(const oscairn_clap_host* host); \
    uint32_t p##clap_note_output_ports(const oscairn_clap_host* host); \
    bool p##clap_uses_clap_notes(const oscairn_clap_host* host, uint32_t port); \
    bool p##clap_takes_sysex(const oscairn_clap_host* host); \
    bool p##clap_emits_sysex(const oscairn_clap_host* host); \
    bool p##clap_supports_state(const oscairn_clap_host* host); \
    bool p##clap_supports_sound_snapshot(const oscairn_clap_host* host); \
    bool p##clap_has_window(const oscairn_clap_host* host); \
    bool p##clap_voice_budget(const oscairn_clap_host* host); \
    bool p##clap_honors_volume(const oscairn_clap_host* host); \
    /* Control thread */ \
    bool p##clap_service(oscairn_clap_host* host); \
    bool p##clap_reset(oscairn_clap_host* host); \
    bool p##clap_reconfigure(oscairn_clap_host* host, double rate, uint32_t frames); \
    int p##clap_effect_resources(const oscairn_clap_host* host, oscairn_effect_resources_t* resources); \
    int p##clap_declared_memory(const oscairn_clap_host* host, oscairn_memory_t* memory); \
    void p##clap_set_memory_room(oscairn_clap_host* host, uint64_t bytes); \
    void p##clap_set_active_outputs(oscairn_clap_host* host, uint64_t ports); \
    void p##clap_set_assets(oscairn_clap_host* host, oscairn_clap_assets* assets); \
    bool p##clap_asset_dependencies(const oscairn_clap_host* host, void (*each)(void* context, const oscairn_clap_asset* asset), void* context); \
    bool p##clap_show_window(oscairn_clap_host* host, bool show, const char* title); \
    bool p##clap_presets(oscairn_clap_host* host, void (*each)(void* context, const oscairn_clap_preset* preset), void* context); \
    bool p##clap_load_preset(oscairn_clap_host* host, const oscairn_clap_preset* preset); \
    /* State, in the directories ClapHost's saveState and saveSound write (StateFiles.h): a new one to save to. */ \
    bool p##clap_save_state(oscairn_clap_host* host, const char* directory); \
    bool p##clap_load_state(oscairn_clap_host* host, const char* directory); \
    /* Parameters */ \
    double p##clap_parameter_value(const oscairn_clap_host* host, uint32_t id); \
    bool p##clap_parameter_text(const oscairn_clap_host* host, uint32_t id, double value, oscairn_clap_text text, void* context); \
    bool p##clap_set_parameter(oscairn_clap_host* host, uint32_t id, double value); \
    bool p##clap_parameter_gesture(oscairn_clap_host* host, uint32_t id, bool begin); \
    uint64_t p##clap_parameter_reports(const oscairn_clap_host* host); \
    bool p##clap_send_event(oscairn_clap_host* host, const oscairn_clap_event* event); \
    bool p##clap_pop_output(oscairn_clap_host* host, oscairn_clap_event* event); \
    uint64_t p##clap_output_event_drops(const oscairn_clap_host* host); \
    void p##clap_statistics(const oscairn_clap_host* host, oscairn_clap_statistics* statistics); \
    /* Native sounds (oscairn.sound-contexts, sound-snapshots), control thread */ \
    uint32_t p##clap_sound_context_capacity(const oscairn_clap_host* host); \
    bool p##clap_sound_contexts(const oscairn_clap_host* host, void (*each)(void* context, const oscairn_sound_context_info_t* info), void* context); \
    bool p##clap_prepare_sound(oscairn_clap_host* host, const char* name, const oscairn_sound_parameter_t* parameters, uint32_t count, \
                               const char* patch, const char* library, const char* directory, uint32_t* id); \
    bool p##clap_prepare_preset(oscairn_clap_host* host, const char* name, const oscairn_clap_preset* preset, uint32_t* id); \
    int p##clap_select_sound(oscairn_clap_host* host, uint32_t id, int16_t channel); \
    int p##clap_discard_sound(oscairn_clap_host* host, uint32_t id); \
    bool p##clap_prepares_snapshots(const oscairn_clap_host* host); \
    bool p##clap_prepare_snapshot(oscairn_clap_host* host, const char* directory, uint32_t ids[16]); \
    bool p##clap_saves_sounds(const oscairn_clap_host* host); \
    bool p##clap_save_sound(oscairn_clap_host* host, uint32_t id, const char* directory); \
    bool p##clap_sound_values(const oscairn_clap_host* host, uint32_t id, void (*each)(void* context, const oscairn_sound_parameter_t* value), void* context); \
    bool p##clap_rings_out(const oscairn_clap_host* host); \
    int p##clap_choke_retiring(oscairn_clap_host* host); \
    /* Audio thread */ \
    bool p##clap_process(oscairn_clap_host* host, const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames, \
                         const oscairn_clap_event* events, uint32_t count, const clap_event_transport_t* transport); \
    const oscairn_clap_event* p##clap_output_events(const oscairn_clap_host* host, uint32_t* count); \
    void p##clap_panic(oscairn_clap_host* host); \
    bool p##clap_asleep(const oscairn_clap_host* host); \
    bool p##clap_restful(const oscairn_clap_host* host); \
    void p##clap_rested(oscairn_clap_host* host, uint32_t frames, uint32_t blocks); \
    void p##clap_mirror_wake(oscairn_clap_host* host, oscairn_clap_wake* mirror); \
    bool p##clap_was_reset(const oscairn_clap_host* host); \
    void p##clap_set_voice_limits(oscairn_clap_host* host, uint32_t limit, const oscairn_voice_budget_context_t* contexts, uint32_t count); \
    uint32_t p##clap_voice_report(const oscairn_clap_host* host, double* cost, const oscairn_voice_budget_note_t** notes, uint32_t* count);

/* new: null when it cannot load, saying why. sound_contexts, sound_values, asset_dependencies, presets: false only
 * when they fail; nothing given then. int returns: 1 true, 0 false, -1 failed. prepare_snapshot: what saveSnapshot
 * wrote for a native engine, prepared beside the playing sounds, the sound each MIDI channel selects into ids.
 * save_sound: one sound as such a snapshot. parameter_text: the text, given once. */
OSCAIRN_CLAP_API(cpp_)

#ifdef __cplusplus
}
#endif
