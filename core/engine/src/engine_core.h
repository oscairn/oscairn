/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* The engine thread's core: note routing (NoteRoute), the voice budget (VoiceBudget, RFC 0018 and
 * 0045) and the budget's per-block bookkeeping, behind one C interface with two implementations:
 * Rust (core/engine/rust, names rust_core_*) and C++ (core_cpp.cpp around NoteRoute.h, cpp_core_*).
 * graph.cpp calls the one OSCAIRN_ENGINE_CORE selects through OSCAIRN_CORE(name); the differential
 * test (core/engine/tests/core_diff.cpp) runs both on the same streams and compares every result.
 *
 * Threads: *_new and *_free on the control thread; everything else on the audio thread, one call at
 * a time. The caller keeps every route and activity a plan names alive until that plan's notes are
 * freed, and never hands one route to two calls at once. Nothing here allocates, locks or logs after
 * *_new. Calls per block: begin and end once; route and finish once per step; voice_limit once per
 * step whose plugin gives up its own voices; route_events once per harmony and per MIDI tap. */
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
    uint64_t faded_for_loss, lost, faded_for_deadline;
} oscairn_activity;

/* A note connection into a step: its ledger, zones (copied), the routes sharing its part's note
 * limit (itself among them; none: it counts alone), the MIDI channel of the shared engine's context
 * it plays (else -1), and whether it is draining (RFC 0021). */
typedef struct
{
    oscairn_route* route;
    const oscairn_zones* zones;
    oscairn_route* const* peers;
    uint32_t peer_count;
    int16_t channel;
    bool draining;
} oscairn_source;
typedef struct { int16_t channel; oscairn_activity* activity; } oscairn_context; /* a shared engine's sound context */
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

#define OSCAIRN_CORE_API(p) \
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
    uint32_t p##notes_voice_limit(const oscairn_notes* notes, uint32_t step, uint32_t voices); \
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
 * notes_voice_limit: oscairn.voice-budget: the voices a step that gives up its own may keep.
 * notes_end: the block is done: the lanes judged against their limits, the timing report kept; slack,
 *   the microseconds left before the callback's deadline once the block was collected (NaN: unknown),
 *   which the Rust core gives notes up for when it runs low. */
OSCAIRN_CORE_API(rust_core_)
OSCAIRN_CORE_API(cpp_core_)

#ifdef __cplusplus
}
#endif
