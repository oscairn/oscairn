/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* The engine thread's core: note routing (NoteRoute), the voice budget (VoiceBudget, RFC 0018 and
 * 0045), the budget's per-block bookkeeping and the plan's audio (buffers, mixing, the output guard,
 * meters), behind one C interface with two implementations:
 * Rust (core/engine/rust, names rust_core_*) and C++ (core_cpp.cpp around NoteRoute.h, cpp_core_*).
 * graph.cpp calls the one OSCAIRN_ENGINE_CORE selects through OSCAIRN_CORE(name); the differential
 * test (core/engine/tests/core_diff.cpp) runs both on the same streams and compares every result.
 *
 * Threads: *_new and *_free on the control thread, audio_buffers on either; everything else on the audio thread, one call at
 * a time. The caller keeps every route and activity a plan names alive until that plan's notes are
 * freed, and never hands one route to two calls at once. Nothing here allocates, locks or logs after
 * *_new. Calls per block: begin and end once; route and finish once per step; voice_limit once per
 * step whose plugin gives up its own voices; route_events once per harmony and per MIDI tap;
 * audio_begin once, audio_mix once per step and once for the outputs, audio_finish once per step. */
#pragma once
#include <oscairn-clap/voice-budget.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#define OSCAIRN_CORE_ALIGN8 alignas(8)
#else
#define OSCAIRN_CORE_ALIGN8 _Alignas(8)
#endif

typedef struct oscairn_route oscairn_route; /* one note connection's ledger */
typedef struct oscairn_notes oscairn_notes; /* one compiled plan's routing and voice budget */

typedef struct { OSCAIRN_CORE_ALIGN8 uint8_t bytes[64]; } oscairn_event; /* a kick::clap::Event */
typedef bool (*oscairn_emit)(void* context, const oscairn_event* event); /* false: no room */

/* NoteZone and NoteZones (note_zone.h), field for field. */
typedef struct
{
    uint8_t low_key, high_key;
    double low_velocity, high_velocity;
    uint16_t channels;
    int8_t transpose;
    double curve, fixed_velocity;
    uint64_t sources;
    uint16_t max_notes;
    uint32_t part;
    uint16_t reserve;
    uint8_t priority, pedals;
} oscairn_zone;
typedef struct { oscairn_zone zones[8]; size_t count; } oscairn_zones;

/* Graph::Activity (graph.h), field for field: a node's counters, read by other threads. */
typedef struct
{
    uint32_t sounding;
    uint64_t quiet, age;
    bool asleep;
    float rest, per_note, seed_rest, seed_note;
    uint64_t stolen, hard_chokes, stolen_for_device;
    float lane_limit, lane_took;
    uint64_t bad_output;
    uint32_t bad_run;
    bool stranded, own_voices, honors_volume;
    float per_note_scale;
    uint32_t voice_limit, over_limit;
    uint64_t faded_for_loss, lost, faded_for_deadline, unfit;
} oscairn_activity;

/* A note connection into a step: its ledger, zones (copied), the routes sharing its part's note
 * limit (itself among them; none: it counts alone), the MIDI channel of the shared engine's context
 * it plays (else -1), whether it is draining (RFC 0021), and the step its events come from (-1: the
 * graph's input). */
typedef struct
{
    oscairn_route* route;
    const oscairn_zones* zones;
    oscairn_route* const* peers;
    uint32_t peer_count;
    int16_t channel;
    bool draining;
    int32_t from;
} oscairn_source;
typedef struct { int16_t channel; oscairn_activity* activity; uint32_t id; } oscairn_context; /* a shared engine's sound context, its node */
/* Where one of a step's parameter-target lanes goes (Graph::mapParamTarget, setControlTargets): the
 * target step and parameter and its range; the target's note connection from this step that per-note
 * lane events address (-1: none); a shared engine's context's channel (-1: none); a control (an index
 * into the run's controls, -1: a plain mapping). */
typedef struct
{
    uint32_t lane;
    int32_t target;
    uint32_t param;
    double minimum, maximum;
    bool stepped;
    int16_t notes, channel;
    int32_t control;
} oscairn_lane_route;
typedef struct { uint32_t step; float gain; } oscairn_link; /* audio to a later step, the largest gain of its routes */
/* A step of the plan, in plan order: its node's activity, its note connections and contexts, and its
 * budget lane: the thread it renders on; in_process: in the engine's process (the callback's time);
 * late_lane: every plugin on its lane renders a block late (a period's time); next: the later steps its
 * audio goes to. */
typedef struct
{
    oscairn_activity* activity;
    const oscairn_source* sources;
    uint32_t source_count;
    const oscairn_context* contexts;
    uint32_t context_count;
    uint32_t lane;
    bool in_process, late_lane;
    const oscairn_link* next;
    uint32_t next_count;
    /* For the run alone: its plugin (an identity) and node; the part's harmony it gets (an index into
     * the run's harmonies, -1: none); its lanes' routes; whether lanes reach it. */
    const void* host;
    uint32_t id;
    int32_t harmony;
    const oscairn_lane_route* lane_routes;
    uint32_t lane_route_count;
    bool lanes_in;
} oscairn_step;

/* One block's timing, microseconds: budget as Graph::setVoiceBudget (0 none, below 0 the load
 * guard alone); a period, the callback's deadline and a worker's allowance; the audio cores; the
 * frames a node stays silent before its released notes are let go (the note-end watchdog); and the
 * callback's slack at or under which its time is running out (notes_end; the Rust core). */
typedef struct
{
    float budget, period, deadline, allowance, cores;
    uint64_t quiet_after;
    float low_slack;
} oscairn_block;

/* An event for a step: from the note source `source` (its zones, peers, the budget; on its channel)
 * or, with after_notes, a parameter lane's per-note event re-addressed to that source's notes (after
 * the notes at its sample); source -1: passed on as it is. tag: where it entered the graph. */
typedef struct
{
    uint32_t time, order;
    const oscairn_event* event;
    int16_t source;
    uint8_t tag;
    bool after_notes;
} oscairn_key;

/* What a step's plugin made of its block: the microseconds it took; whether it rendered, whether its
 * notes are gone (a lost block, or its host reset it), and whether its block was late or silenced (a
 * worker past its allowance); its oscairn.voice-budget report
 * (read when the budget counts its notes and it rendered); the events it sent (its NOTE_ENDs). */
typedef struct
{
    float took;
    bool processed, reset, missed, own_voices, honors_volume;
    uint32_t voices;
    double voice_cost;
    const oscairn_voice_budget_note_t* notes;
    uint32_t note_count;
    const oscairn_event* sent;
    uint32_t sent_count;
} oscairn_finish;

/* A plan's audio (oscairn_audio): its buffers, the audio connections into its steps and the graph's
 * outputs, the output guard, meters and the activity counters a step's sound keeps. A connection
 * reads channel `from` of step `source`'s outputs (-1: the external input) into channel `to`, at
 * `gain`, scaled by the live gains its slots name (0: none; Graph::setLiveGain). */
typedef struct { int32_t source; uint32_t from, to; float gain; uint8_t live[4]; } oscairn_audio_route;
typedef uint32_t oscairn_peaks[16]; /* a node's meter: Graph::meterChannels floats, as atomics */
/* A shared engine's sound context: its bus (channels from offset), its meter and its activity. */
typedef struct { uint32_t offset, channels; oscairn_peaks* peaks; oscairn_activity* activity; } oscairn_bus;
/* A step, in plan order: its plugin (an identity: a played change's gains ramp between plans), its
 * channels in and out, how many of those out its own meter shows (from the first), its meter and
 * activity (either may be null), the connections into it and its sound contexts' buses. */
typedef struct
{
    const void* host;
    uint32_t inputs, outputs, metered;
    oscairn_peaks* peaks;
    oscairn_activity* activity;
    const oscairn_audio_route* routes;
    uint32_t route_count;
    const oscairn_bus* buses;
    uint32_t bus_count;
} oscairn_audio_step;
typedef struct oscairn_audio oscairn_audio;

/* A plan's block (oscairn_run, Graph::process): around its notes and audio, the steps in plan order
 * wave by wave, each step's events merged and routed, its plugin called, what it sent passed on
 * (parameter-target lanes, MIDI taps, listeners), the parts' harmonies. */
typedef struct oscairn_run oscairn_run;
/* Graph::ControlCurve and ControlTarget, field for field (the path's curves, from the control down). */
typedef struct { uint8_t kind; double start, end, shift, low, high, exponent; } oscairn_curve;
typedef struct { const oscairn_curve* path; uint32_t count; double origin; bool modulate; } oscairn_control;
/* A part's harmony (Graph::setHarmonies): its keys' ledger and zones, whether its input's notes
 * play into it, the key and scale, and the last harmony sent (the caller's, kept across plans). */
typedef struct { oscairn_route* route; const oscairn_zones* zones; bool input; uint8_t root; uint16_t scale; struct oscairn_harmony* last; } oscairn_harmony_source;
/* A MIDI tap (Graph::setMidiTaps): from a step's notes (-1: the input), its ledger and zones, ports and channel. */
typedef struct { int32_t step; oscairn_route* route; const oscairn_zones* zones; uint64_t ports; uint8_t channel; } oscairn_tap;
typedef struct { uint32_t time; uint64_t ports; uint8_t size; uint8_t bytes[3]; } oscairn_midi; /* Graph::MidiMessage */
typedef struct { uint32_t node; oscairn_event event; } oscairn_param; /* a queued parameter's event for a node */
typedef void (*oscairn_job)(void* context, uint32_t index);
/* The plugins, for the run: each call names a step; `context` is passed back. Graph::process's calls
 * on PluginHost, a SoundContext and the Listener, one each, the clock (steady_clock nanoseconds), the
 * worker deadline (realtime.h), and the audio helpers (render runs on them; nothing else does). */
typedef struct
{
    void* context;
    const oscairn_event* (*sent)(void* context, uint32_t step, uint32_t* count);
    bool (*start)(void* context, uint32_t step, const oscairn_event* events, uint32_t count);
    bool (*render)(void* context, uint32_t step, const oscairn_event* events, uint32_t count);
    bool (*finish)(void* context, uint32_t step);
    bool (*late)(void* context, uint32_t step);
    void (*panic)(void* context, uint32_t step);
    bool (*kept)(void* context, uint32_t step);
    bool (*was_reset)(void* context, uint32_t step);
    bool (*asleep)(void* context, uint32_t step);
    bool (*own_voices)(void* context, uint32_t step);
    bool (*honors_volume)(void* context, uint32_t step);
    uint64_t (*missed)(void* context, uint32_t step);
    double (*took)(void* context, uint32_t step);
    uint32_t (*voices)(void* context, uint32_t step, double* cost, const oscairn_voice_budget_note_t** notes, uint32_t* count);
    void (*set_limits)(void* context, uint32_t step, uint32_t limit, const oscairn_voice_budget_context_t* contexts, uint32_t count);
    uint32_t (*sound)(void* context, uint32_t step, uint32_t context_index);
    void (*parameters)(void* context, uint32_t step, uint32_t context_index, oscairn_emit emit, void* emit_context);
    void (*listen)(void* context, uint32_t node, const oscairn_event* events, uint32_t count, bool sent);
    int64_t (*now)(void* context);
    void (*deadline)(void* context, int64_t at, bool only);
    void (*helpers)(void* context, uint32_t count, oscairn_job job, void* job_context);
} oscairn_plugins;
/* One block for the run: the graph's audio in and out, frames, and the output guard's fade (frames); its events (each's tag in tags, which may be
 * null) and the queued parameters; the beat event; recovery asked for the input's notes or all; whether
 * a listener listens and the plan renders wave by wave (else step by step on this thread); the budget's
 * timing; the callback's deadline (nanoseconds, 0: none) and whether waves wait to it (RFC 0045 step
 * 6); what the helpers must be saved to be worth waking; the live gains; the graph's note IDs, event
 * drops (an atomic) and MIDI out (count in and out). */
typedef struct
{
    const float* const* inputs;
    uint32_t ni;
    float* const* outputs;
    uint32_t no, frames, fade;
    const oscairn_event* events;
    uint32_t count;
    const uint8_t* tags;
    const oscairn_param* params;
    uint32_t param_count;
    const oscairn_event* beat;
    bool panic_input, panic_notes, listening, parallel, deadlines;
    oscairn_block timing;
    int64_t deadline;
    float worth;
    const float* live_from;
    const float* live_to;
    uint32_t* next_id;
    uint64_t* drops;
    oscairn_midi* midi;
    uint32_t* midi_count;
} oscairn_run_block;

#define OSCAIRN_CORE_API(p) \
    oscairn_audio* p##audio_new(const oscairn_audio_step* steps, uint32_t count, const oscairn_audio_route* outputs, uint32_t output_count, \
                                uint32_t inputs, uint32_t channels, uint32_t max_frames); \
    void p##audio_free(oscairn_audio* audio); \
    float* const* p##audio_buffers(oscairn_audio* audio, int32_t step, bool outputs); \
    void p##audio_carry(oscairn_audio* next, const oscairn_audio* current, uint32_t ramp); \
    void p##audio_begin(oscairn_audio* audio, const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames); \
    void p##audio_mix(oscairn_audio* audio, uint32_t step, float* const* outputs, uint32_t frames, const float* live_from, const float* live_to); \
    void p##audio_finish(oscairn_audio* audio, uint32_t step, uint32_t frames, bool processed, bool asleep, uint32_t fade); \
    oscairn_run* p##run_new(oscairn_notes* notes, oscairn_audio* audio, const oscairn_step* steps, const bool* budgeted, uint32_t count, \
                            const uint32_t* waves, uint32_t wave_count, \
                            const oscairn_harmony_source* harmonies, uint32_t harmony_count, const oscairn_tap* taps, uint32_t tap_count, \
                            const oscairn_control* controls, uint32_t control_count); \
    void p##run_free(oscairn_run* run); \
    void p##run_adopt(oscairn_run* next, oscairn_run* current, bool ramp, uint32_t ramp_frames, oscairn_midi* midi, uint32_t* midi_count, uint64_t* drops); \
    bool p##run_block(oscairn_run* run, const oscairn_run_block* block, const oscairn_plugins* plugins); \
    oscairn_route* p##route_new(uint32_t ports, uint16_t clap_ports, uint32_t fade_frames); \
    void p##route_free(oscairn_route* route); \
    uint32_t p##route_live(const oscairn_route* route); \
    void p##route_pedals(const oscairn_route* route, uint8_t pedals[256]); \
    void p##route_held_keys(const oscairn_route* route, uint64_t keys[2]); \
    bool p##route_retire(oscairn_route* route, const uint8_t remaining[256], oscairn_emit emit, void* context); \
    void p##route_let_go(oscairn_route* route, oscairn_emit emit, void* context); \
    void p##route_events(oscairn_route* route, const oscairn_zones* zones, const oscairn_event* events, uint32_t count, \
                         const uint8_t* tags, bool skip_ends, uint32_t* next_id, oscairn_emit emit, void* context, uint64_t* held); \
    oscairn_notes* p##notes_new(const oscairn_step* steps, uint32_t count, uint32_t lanes, bool* budgeted); \
    void p##notes_free(oscairn_notes* notes); \
    void p##notes_begin(oscairn_notes* notes, const oscairn_block* block); \
    uint32_t p##notes_route(oscairn_notes* notes, uint32_t step, oscairn_key* keys, uint32_t count, uint32_t frames, uint32_t* next_id, \
                            oscairn_event* merged, uint32_t capacity, int16_t* heard, bool* overflow); \
    void p##notes_finish(oscairn_notes* notes, uint32_t step, const oscairn_finish* finish); \
    uint32_t p##notes_voice_limit(const oscairn_notes* notes, uint32_t step, uint32_t voices, uint32_t* contexts, uint32_t count); \
    void p##notes_end(oscairn_notes* notes, float slack);

/* route_new: a ledger for a destination with `ports` note ports, bit p of clap_ports set where port p
 *   takes CLAP notes (else MIDI), and a stolen note's fade of fade_frames (0: choked at once).
 * route_pedals, route_held_keys: its sustain pedals [port][channel], and the keys its notes hold.
 * route_retire: the connection leaves the plan: its CLAP notes choked, its MIDI channels silenced,
 *   its pedals set to `remaining` (the routes staying), then emptied. False if a release found no room.
 * route_let_go: a MIDI tap gone: a release for each note, its pedals lifted, then emptied.
 * route_events: events through the route's zones alone (no peers, no budget): those in the core
 *   space, NOTE_END too unless skip_ends; tags may be null (untagged); held, when given, gets the keys
 *   held after each event routed (two words at 2 * its index).
 * notes_new: a plan's note state; budgeted gets, per step, whether the budget counts its notes.
 * notes_begin: a block starts: the lanes' limits from its timing.
 * notes_route: one step's events this block, sorted in place (by time, per-note parameter events
 *   after the notes at their sample, then order) and routed with the stolen notes' fades among them
 *   into merged (each event's channel as it came into heard, when given). overflow: in, the block is
 *   already lost; out, whether it was: then its routes are emptied and nothing is merged. Returns the
 *   events merged.
 * notes_finish: after the step's plugin rendered: its notes counted and costed, a lost block marked on
 *   its lane and the lanes before it (the Rust core), its NOTE_ENDs taken.
 * notes_voice_limit: oscairn.voice-budget: the voices a step that gives up its own may keep, `voices`
 *   sounding now: those of the notes admitted and the room left at its cost per voice, never fewer than
 *   those admitted unless its lane is measured over its limit (then at most a quarter fewer a block);
 *   and into `contexts` (`count`, the step's sound contexts in order) each context's: its admitted
 *   voices and its share of the instance's room, 0 for one holding none.
 * notes_end: the block is done: the lanes judged against their limits, the timing report kept; slack,
 *   the microseconds left before the callback's deadline once the block was collected (NaN: unknown),
 *   which the Rust core gives notes up for when it runs low.
 * audio_new: a plan's audio, every buffer max_frames long: `inputs` external input channels, each
 *   step's; the graph's own `channels` out are the caller's. Connections to one channel add up, in order.
 * audio_buffers: a step's input or output channels (the plugin's buffers), or with step -1 the external input.
 * audio_carry: a played change: a connection in both plans (same plugins, same channels) ramps over
 *   `ramp` frames from the gain it has in `current` to its gain in `next`.
 * audio_begin: a block starts: the external input copied in (a channel missing: silence), the graph's
 *   outputs no connection reaches and those past its own silenced.
 * audio_mix: a step's inputs mixed from its connections, or with step == count the graph's outputs
 *   from theirs into `outputs`; live_from and live_to, the live gains at the block's start and end.
 * audio_finish: after the step's plugin rendered: the output guard, when it rendered (a sample not
 *   finite or past Graph::outputBound fades the block from the last good samples over `fade` frames,
 *   counted as bad output); its meters; and its and its contexts' quiet, asleep and age.
 * run_new: a plan's block over its notes and audio (which outlive it), every array copied; budgeted:
 *   notes_new's answer for each step; waves: where each wave of steps ends.
 * run_adopt: `next` replaces `current` (Graph::beginBlock): the connections leaving the plan retired
 *   into the next plan's steps' releases, plugins leaving it holding notes marked stranded and those
 *   coming back choked, the taps gone let go into midi, and with `ramp` the gains carried.
 * run_block: the block (Graph::process from the beat on); false where a plugin did not render. */
OSCAIRN_CORE_API(rust_core_)
OSCAIRN_CORE_API(cpp_core_)

#ifdef __cplusplus
}
#endif
