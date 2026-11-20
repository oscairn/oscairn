/* SPDX-License-Identifier: MIT */
#pragma once
#include <clap/clap.h>
#ifdef __cplusplus
extern "C" {
#endif

/*
 * oscairn.voice-budget (draft 2, and draft 1 without `importance`): a plugin that gives up its own voices within a limit the host
 * sets, so the host need not choose among its notes (RFC 0018, option C). The host shares one
 * processing budget among every plugin; it measures each plugin's cost, and for a plugin with
 * this extension it reads what the plugin plays and tells it how many voices it may keep.
 *
 * A voice is whatever the plugin renders and can give up on its own: a note's one oscillator, one
 * unison copy, one layer, or one element or region of a sample-based sound. A note may sound with
 * several voices; giving one up need not end the note. The plugin fades what it gives up (2 to 5
 * ms is enough), never cuts it.
 *
 * Per sound context (oscairn.sound-contexts), which is per part: a plugin playing several parts'
 * sounds from one pool of voices (one instance, several contexts) gets each context's limit,
 * reserve and priority, and whether its part plays mono or legato, and chooses among its voices
 * accordingly (the order below). A plugin without sound contexts reads the entry for
 * OSCAIRN_VOICE_BUDGET_SELECTED: its one sound is the selected one (voice-budget-plugin.hpp's
 * Limits::of(0, true)).
 * (Oscairn: parts sharing one instance, RFC 0021, get an entry each, for the sound their channel
 * selects, with the part's reserve and priority; an instance one part plays alone gets one entry,
 * for the selected contexts, which also covers a Performance change, the outgoing sounds ringing
 * out beside the incoming ones. No mode flags, since parts have no mono or legato mode yet, and
 * no limit of a context's own: the host keeps each part's note limit itself, RFC 0018 step 2.)
 *
 * Calls, all optional to act on but required in the table (no null entries):
 *
 *   honors_volume [main-thread]: whether every voice follows CLAP_NOTE_EXPRESSION_VOLUME, so
 *   the host's own 5 ms fade before a choke (when it gives up a whole note at its hard limit)
 *   is known to work.
 *
 *   set_limits [audio-thread, never during process()]: the voices the instance may keep from the
 *   next process() on (0: as many as its pool holds), and the settings of up to
 *   OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS contexts. The host calls it between blocks whenever any of
 *   it changes; the plugin copies what it needs. It does not wake a sleeping plugin: the limits
 *   apply on the next block that runs. Past the limit, the plugin gives up voices at
 *   the start of the next block, in its own order; a new note past it takes a voice the same way.
 *
 *   active_voices [audio-thread, after process()]: the voices sounding now (fading ones included;
 *   in draft 2 the units importance lists, so notes for an engine that gives a note up only whole),
 *   and the plugin's estimate of one voice's cost in microseconds a block at the current block
 *   size (0: no estimate). The host measures the instance's time either way, and uses the count
 *   to share it per voice rather than per note.
 *
 *   notes [audio-thread, after process()]: each note still sounding, by the address the host gave
 *   it (note ID, port, channel, key), with how many voices sound it and how loud it is now: the
 *   largest of its voices' current gains, linear, 1 for a voice at full level (its envelope,
 *   velocity and volume expression; not the patch's output level). Writes at most `capacity` and
 *   returns how many it wrote. The host compares these across plugins to give up the least
 *   audible notes first.
 *
 *   importance [audio-thread, after process()] (draft 2; RFC 0046): for each context sounding, its
 *   OSCAIRN_VOICE_BUDGET_LEAST least important voices in the order below, least important first,
 *   and how many voices it sounds; at most `capacity` contexts, returns how many it wrote. A voice
 *   rule 1 keeps is not listed. When the host then lowers a context's limit by n, the plugin gives
 *   up the n voices it listed first, at the start of its next block: the host checks it. So the
 *   plugin MUST tell each listed unit from a later one in the same slot (the SDK helper does it
 *   by a note-on serial, never a frame stamp or a time).
 *   A listed voice is the plugin's unit of giving up, the same thing its limit and active_voices
 *   count (a sampler's note with all its layers, a synth's voice), never a part of one.
 *   Units, not weight, are what is counted: `voices`, a context's limit, the instance limit and the
 *   n the host lowers a limit by all count units, and each entry is one unit, so `limit - n` is
 *   exact. Weight is only the host's estimate of a unit's cost: each entry's `weight` is its unit's
 *   cost relative to the others (the layers or oscillators it runs; 0 counts as 1), and a
 *   context's `weight` the sum over the units it counts in `voices` (0: `voices`, every unit 1).
 *   The host turns weight into time: the instance's measured time above rest divided by its
 *   contexts' total weight, times the entry's weight. So the unit must be what the limit counts.
 *   An engine whose notes hold several engine voices it does not give up one by one (unison
 *   copies, a sampler's layers) counts notes: one entry per note, its `weight` the engine voices
 *   or layers the note runs, and its limits and active_voices in notes. An engine that gives up
 *   one layer or copy alone may count those instead, each an entry of its own.
 *   `age_ms` and the struck flag are taken at the end of the block the report follows (the
 *   instant the next block starts, when a lowered limit applies).
 *   `level_db` is cheap and the same in every plugin: 20 log10 of the product of the voice's
 *   envelope level, its velocity gain, its volume expression and the sound's own output level, 0
 *   dB being the voice's sample or oscillator at its nominal full scale; no pass over its samples.
 *   The host adds the part's gain. Tier, the struck flag and age are carried apart from it.
 *
 * The order a plugin gives up voices in, which the Oscairn SDK's helper (voice-budget-plugin.hpp)
 * implements (RFC 0046's scale, the same in every plugin):
 *   1. never a held voice of a context marked OSCAIRN_VOICE_BUDGET_MONO or _LEGATO (its one key,
 *      a legato line's included), nor a held voice of a context at or under its reserve, unless
 *      the new voice is for that same context;
 *   2. by tier: released (key up, no pedal holding it) or inaudible (below -60 dBFS at the
 *      plugin's output, unless struck under 50 ms ago: an attack is not judged by its level), then
 *      held only by a pedal, then key held;
 *   3. the new voice's own context before others;
 *   4. the context of lower priority before higher;
 *   5. a voice struck under 50 ms ago after the others of its tier; then the least audible: its
 *      level in dBFS less a decibel per second of age (at most ten), then the oldest.
 * A plugin may refine this (one element of a layered note before a whole note) but should not
 * break rule 1.
 *
 * Draft 2 adds `importance` to the table under its own ID, OSCAIRN_EXT_VOICE_BUDGET_DRAFT_2;
 * OSCAIRN_EXT_VOICE_BUDGET stays draft 1's, so an engine rebuilt with a four-entry table keeps
 * answering what it always did. A plugin offering draft 2 returns the same table for both IDs. A
 * host asks for draft 2 first, then draft 1, and treats a draft-1 table, or a draft-2 table whose
 * `importance` is null, as reporting no importance (the cost-only behavior), never refusing it.
 */

static const char OSCAIRN_EXT_VOICE_BUDGET[] = "oscairn.voice-budget/draft-1";
static const char OSCAIRN_EXT_VOICE_BUDGET_DRAFT_2[] = "oscairn.voice-budget/draft-2";

enum
{
    OSCAIRN_VOICE_BUDGET_MONO = 1 << 0,   // the part plays one note at a time
    OSCAIRN_VOICE_BUDGET_LEGATO = 1 << 1, // and glides between notes without retriggering
};

// The context set_limits names when it means every context a MIDI channel selects (the part's own
// sounds), apart from those it names by ID.
static const uint32_t OSCAIRN_VOICE_BUDGET_SELECTED = CLAP_INVALID_ID;
static const uint32_t OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS = 16;

typedef struct oscairn_voice_budget_context
{
    uint32_t context;  // a sound context's ID, or OSCAIRN_VOICE_BUDGET_SELECTED
    uint32_t limit;    // voices it may keep; 0: no limit of its own
    uint32_t reserve;  // voices never given up for another context's
    uint8_t  priority; // 0 to 10: a context of lower priority gives way first
    uint8_t  flags;    // OSCAIRN_VOICE_BUDGET_MONO, OSCAIRN_VOICE_BUDGET_LEGATO
} oscairn_voice_budget_context_t;

typedef struct oscairn_voice_budget_note
{
    int32_t  note_id;
    int16_t  port_index, channel, key;
    uint32_t voices;   // voices sounding it now
    float    loudness; // linear, the largest of its voices' gains now
} oscairn_voice_budget_note_t;

enum
{
    OSCAIRN_VOICE_BUDGET_LEAST = 16, // the voices a context's importance report lists
    OSCAIRN_VOICE_BUDGET_STRUCK = 1, // a listed voice's flag: struck under 50 ms ago
};

// A voice in an importance report (draft 2).
typedef struct oscairn_voice_budget_least
{
    uint8_t  tier;     // 0 released or inaudible, 1 held only by a pedal, 2 key held
    uint8_t  flags;    // OSCAIRN_VOICE_BUDGET_STRUCK
    uint16_t age_ms;   // since its note-on, at the end of the block the report follows; at most 65,535
    float    level_db; // its gain now at the plugin's output, dBFS (envelope, velocity, volume expression and
                       // the sound's own output level), -120 to +24 (clamped there); -120 for silence
    float    weight;   // its cost relative to the context's other voices (layers, oscillators); 0 counts as 1;
                       // the host's cost estimate only: the entry is one unit of the limit whatever its weight
} oscairn_voice_budget_least_t;

// A context's importance report (draft 2).
typedef struct oscairn_voice_budget_importance
{
    uint32_t context; // a sound context's ID; a plugin without sound contexts lists one, under the ID it describes
                      // its voices with (the SDK's examples use 0, its one sound, whose settings Limits::of(0, true) reads)
    uint16_t voices;  // its voices sounding, those fading out not counted
    uint16_t count;   // entries in least
    float    weight;  // the sum of its sounding voices' weights; 0: voices (each 1)
    oscairn_voice_budget_least_t least[OSCAIRN_VOICE_BUDGET_LEAST]; // least important first
} oscairn_voice_budget_importance_t;

typedef struct oscairn_plugin_voice_budget
{
    bool (CLAP_ABI* honors_volume)(const clap_plugin_t* plugin);
    void (CLAP_ABI* set_limits)(const clap_plugin_t* plugin, uint32_t limit, const oscairn_voice_budget_context_t* contexts, uint32_t count);
    uint32_t (CLAP_ABI* active_voices)(const clap_plugin_t* plugin, double* voice_cost);
    uint32_t (CLAP_ABI* notes)(const clap_plugin_t* plugin, oscairn_voice_budget_note_t* notes, uint32_t capacity);
    // Draft 2 only: absent from a draft-1 table; null in a draft-2 table means no report.
    uint32_t (CLAP_ABI* importance)(const clap_plugin_t* plugin, oscairn_voice_budget_importance_t* contexts, uint32_t capacity);
} oscairn_plugin_voice_budget_t;

#ifdef __cplusplus
}
#endif
