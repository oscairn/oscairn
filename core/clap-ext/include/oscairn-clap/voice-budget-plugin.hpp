// SPDX-License-Identifier: MIT
//
// Part of the Oscairn plugin SDK (MIT, like the extension headers beside it).
//
// oscairn.voice-budget (draft 2, voice-budget.h) for a plugin's own pool of voices: the settings
// the host gives (Limits), the order to give voices up in (victim), the report of each note's
// loudness (Report) and of each context's least important voices (Importance). Header-only, no
// allocation; everything here runs on the audio thread.
//
// An instrument built on sound-contexts-plugin.hpp gets all of it from its Engine (see there).
// Any other engine, such as one playing several regions or elements per note, uses it so:
//   - keep a Limits; set_limits calls limits.set(limit, contexts, count) (never during process);
//   - when a new voice is needed, or at a block's start while more voices sound than the limit
//     (or a context's own limit) allows, describe each voice as a Candidate and call victim();
//     fade the voice it names out (2 to 5 ms) rather than cutting it, unless the pool has no
//     free slot at all;
//   - for active_voices, count the voices sounding; for notes, add each voice to a Report and
//     write it out;
//   - after each block, Importance::list() over the same Candidates victim() sees, and write it
//     out from importance(). Fill each Candidate's pedaled and ageMs (since its note-on, from the
//     plugin's own frame count) as well as held and loudness (its gain at the plugin's output,
//     the sound's output level included), so the host ranks it on the same scale as every other
//     plugin's (RFC 0046); and its age, which MUST be a note-on serial unique to each note a slot
//     holds (never a frame stamp or a time: two notes started on one frame would share it), since
//     next() tells a listed voice from a later one in its slot by it. At a block's start, past a lowered limit, give up Importance::next()'s
//     voice rather than victim()'s: the voices list() named, in the order it named them, while
//     they still sound in their slots, then victim()'s. So a context the host lowers by n gives up
//     exactly the n voices it listed first, though levels, ages and the 50 ms mark have moved
//     since the list was made.
// victim() gives up voices one at a time. An engine that can give up one layer of a note alone may
// keep each layer a Candidate with its own loudness, so a layered note loses its quietest layer
// before another note loses its only one; an engine that gives a note up only whole (unison
// copies, layers it cannot drop alone) makes the note one Candidate, its weight the engine voices
// or layers it runs, and counts its limits and active_voices in notes.

#pragma once
#include <oscairn-clap/voice-budget.h>

#include <algorithm>
#include <array>
#include <cassert>
#include <cmath>
#include <cstddef>
#include <cstdint>

namespace oscairn::voices
{
    // The host's settings, as set_limits last gave them.
    struct Limits
    {
        uint32_t limit = 0; // voices for the instance; 0: as many as the pool holds
        std::array<oscairn_voice_budget_context_t, OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS> contexts{};
        uint32_t count = 0;
        void set(uint32_t voices, const oscairn_voice_budget_context_t* list, uint32_t n) noexcept
        {
            limit = voices; count = list ? std::min<uint32_t>(n, OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS) : 0;
            for(uint32_t i = 0; i < count; ++i) contexts[i] = list[i];
        }
        // A context's settings: its own entry; else, for one a channel selects, the entry for the
        // selected contexts; else none (no limit or reserve, priority 0).
        oscairn_voice_budget_context_t of(uint32_t id, bool selected) const noexcept
        {
            const oscairn_voice_budget_context_t* found = nullptr;
            for(uint32_t i = 0; i < count; ++i)
            {
                if(contexts[i].context == id && id != OSCAIRN_VOICE_BUDGET_SELECTED) return contexts[i];
                if(selected && contexts[i].context == OSCAIRN_VOICE_BUDGET_SELECTED) found = &contexts[i];
            }
            if(found) return *found;
            oscairn_voice_budget_context_t none{}; none.context = id; return none;
        }
    };

    // RFC 0046's scale, the same in every plugin: a voice's tier, then its audibility.
    constexpr double struckMs = 50;        // a voice struck this recently is not judged by its level, and goes last in its tier
    constexpr double inaudibleDb = -60;    // below this at the plugin's output a voice counts as released
    constexpr double agedDbPerSecond = 1;  // a voice counts this much quieter for each second of its age,
    constexpr double agedSecondsMost = 10; // up to this many
    enum Tier : uint8_t { released = 0, pedaled = 1, keyHeld = 2 };

    inline float levelDb(double gain) noexcept
    { return gain > 1e-6 ? float(std::clamp(20 * std::log10(gain), -120.0, 24.0)) : -120.f; }

    // A voice as the order sees it: the plugin's unit of giving up, what its limit counts (a
    // sampler's note with its layers, a synth's voice). ending: already fading out (a choke or a
    // steal), so giving it up frees nothing new; mono: the plugin's own sound plays mono, whatever
    // the host says. held: its key is down; pedaled: its key is up and a pedal holds it. loudness:
    // its gain now at the plugin's output, linear: envelope level x velocity gain x volume
    // expression x the sound's output level, 1 for its sample or oscillator at nominal full scale
    // (no pass over its samples). ageMs: since its note-on, below 0 when the plugin does not keep
    // time; for the report, at the end of the block just rendered. weight: its cost relative to the
    // plugin's other voices (the layers or oscillators it runs), the host's cost estimate only: a
    // Candidate is one unit of the limit whatever its weight.
    struct Candidate
    {
        uint32_t context = 0;
        bool selected = false, held = false, ending = false, mono = false;
        double loudness = 0;
        uint64_t age = 0; // larger: newer; a note-on serial, unique to each note a slot holds (next() needs it)
        bool pedaled = false;
        double ageMs = -1;
        float weight = 1;

        bool struck() const noexcept { return ageMs >= 0 && ageMs < struckMs; }
        uint8_t tier() const noexcept
        {
            if(!struck() && levelDb(loudness) < inaudibleDb) return released;
            return held ? keyHeld : pedaled ? Tier::pedaled : released;
        }
        // Its level less its age's share: the larger, the more it is heard.
        double audibility() const noexcept
        { return levelDb(loudness) - (ageMs > 0 ? std::min(ageMs / 1000 * agedDbPerSecond, agedSecondsMost * agedDbPerSecond) : 0); }
    };

    constexpr uint32_t none = OSCAIRN_VOICE_BUDGET_SELECTED; // victim()'s `incoming` when no new voice waits

    // Whether a goes before b within one context: the lower tier, one not just struck, the less audible, the older.
    inline bool before(const Candidate& a, const Candidate& b) noexcept
    {
        const auto ta = a.tier(), tb = b.tier();
        if(ta != tb) return ta < tb;
        if(a.struck() != b.struck()) return b.struck();
        const double xa = a.audibility(), xb = b.audibility();
        return xa != xb ? xa < xb : a.age < b.age;
    }

    // Each context's held voices, for its reserve.
    struct HeldCount
    {
        struct Held { uint32_t context = 0, count = 0; };
        std::array<Held, 64> held{}; size_t contexts = 0;
        uint32_t& of(uint32_t context) noexcept
        {
            for(size_t i = 0; i < contexts; ++i) if(held[i].context == context) return held[i].count;
            if(contexts == held.size()) return held[held.size() - 1].count; // shortcut: past 64 contexts the last slot is shared; no Oscairn plugin has that many
            held[contexts].context = context; return held[contexts++].count;
        }
        template<class Get> HeldCount(size_t count, Get&& get)
        { for(size_t i = 0; i < count; ++i) if(const Candidate* c = get(i); c && c->held && !c->ending) ++of(c->context); }
    };

    // Rule 1: a voice the order never gives up for a new voice in `incoming` (or none).
    inline bool kept(const Candidate& c, const Limits& limits, HeldCount& held, uint32_t incoming) noexcept
    {
        const auto settings = limits.of(c.context, c.selected);
        if(c.held && (c.mono || (settings.flags & (OSCAIRN_VOICE_BUDGET_MONO | OSCAIRN_VOICE_BUDGET_LEGATO)))) return true; // never a held mono key
        return c.held && c.context != incoming && held.of(c.context) <= settings.reserve; // its reserve
    }

    // The voice to give up, by voice-budget.h's order, among `count` voices that get(i) describes
    // (a Candidate, or nullptr for a free slot): for a new voice in context `incoming` (or `none`),
    // from context `only` alone when it is not `none` (to keep that context within its own limit).
    // -1 when every voice is protected or ending.
    template<class Get>
    int victim(size_t count, Get&& get, const Limits& limits, uint32_t incoming, uint32_t only = none)
    {
        HeldCount held(count, get);
        int best = -1; const Candidate* chosen = nullptr; int bestOther = 0; uint8_t bestPriority = 0;
        for(size_t i = 0; i < count; ++i)
        {
            const Candidate* c = get(i);
            if(!c || c->ending || (only != none && c->context != only) || kept(*c, limits, held, incoming)) continue;
            const auto settings = limits.of(c->context, c->selected);
            const int other = incoming != none && c->context != incoming ? 1 : 0;
            const auto tier = c->tier(), bestTier = chosen ? chosen->tier() : uint8_t(0);
            const bool better = !chosen || (tier != bestTier ? tier < bestTier : other != bestOther ? other < bestOther :
                settings.priority != bestPriority ? settings.priority < bestPriority : before(*c, *chosen));
            if(better) { best = int(i); chosen = c; bestOther = other; bestPriority = settings.priority; }
        }
        return best;
    }

    // draft 2's importance report: each context's least important voices in victim()'s order with no
    // voice incoming, so a context lowered by n gives up the n it listed first. Build it after each
    // block with list(), then write it out from importance(). One pass over the voices, no sorting:
    // each voice's key is worked out once (one log10) and kept only if it beats the 16th kept, by
    // insertion into the 16 slots.
    template<size_t Contexts = OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS>
    struct Importance
    {
        // A kept voice with its key worked out, and its slot, for next().
        struct Ranked
        {
            Candidate c{};
            size_t index = 0;
            uint8_t tier = 0; bool struck = false; float db = -120; double audibility = 0;
            bool before(const Ranked& b) const noexcept
            {
                if(tier != b.tier) return tier < b.tier;
                if(struck != b.struck) return b.struck;
                return audibility != b.audibility ? audibility < b.audibility : c.age < b.c.age;
            }
        };
        struct List { oscairn_voice_budget_importance_t out{}; std::array<Ranked, OSCAIRN_VOICE_BUDGET_LEAST> least{}; };
        std::array<List, Contexts> lists{};
        uint32_t count = 0;
        bool truncated = false; // the last list() or write() left a context out: past Contexts, or past the host's capacity

        template<class Get>
        void list(size_t voices, Get&& get, const Limits& limits)
        {
            count = 0; truncated = false;
            HeldCount held(voices, get);
            for(size_t i = 0; i < voices; ++i)
            {
                const Candidate* c = get(i);
                if(!c || c->ending) continue;
                List* l = find(c->context);
                if(!l) { truncated = true; continue; }
                ++l->out.voices;
                l->out.weight += c->weight > 0 ? c->weight : 1.f;
                if(kept(*c, limits, held, none)) continue;
                insert(*l, rank(*c, i));
            }
            // Past its reserve a context lists only the held voices beyond it: the least important of them.
            for(uint32_t k = 0; k < count; ++k)
            {
                auto& l = lists[k];
                const auto settings = limits.of(l.out.context, l.least[0].c.selected);
                const uint32_t spare = held.of(l.out.context) > settings.reserve ? held.of(l.out.context) - settings.reserve : 0;
                uint32_t n = 0, heldListed = 0;
                for(uint32_t i = 0; i < l.out.count; ++i)
                    if(!l.least[i].c.held || heldListed++ < spare) l.least[n++] = l.least[i];
                l.out.count = uint16_t(n);
                for(uint32_t i = 0; i < n; ++i)
                {
                    const auto& r = l.least[i];
                    l.out.least[i] = {r.tier, uint8_t(r.struck ? OSCAIRN_VOICE_BUDGET_STRUCK : 0), uint16_t(r.c.ageMs >= 0 ? std::min(r.c.ageMs, 65535.0) : 0.0) /* NaN or unknown: 0 */, r.db, r.c.weight > 0 ? r.c.weight : 1.f};
                }
            }
        }
        // The voice to give up at a block's start, past a limit the host lowered (from context `only`
        // alone unless none): the first voice the last list() named that still sounds in its slot (the
        // same age, not fading out) and that rule 1 does not keep now; a context's in its listed order,
        // across contexts the first of each by victim()'s order (tier, then priority, then the key). When none is
        // left, victim()'s. -1 when no voice may go.
        template<class Get>
        int next(size_t voices, Get&& get, const Limits& limits, uint32_t only = none)
        {
            HeldCount held(voices, get);
            const Ranked* best = nullptr; uint8_t bestPriority = 0;
            for(uint32_t k = 0; k < count; ++k)
            {
                const auto& l = lists[k];
                if(only != none && l.out.context != only) continue;
                for(uint32_t e = 0; e < l.out.count; ++e)
                {
                    const Ranked& r = l.least[e];
                    const Candidate* c = r.index < voices ? get(r.index) : nullptr;
                    if(!c || c->ending || c->age != r.c.age || c->context != r.c.context || kept(*c, limits, held, none)) continue; // ended, replaced or kept since
                    // Debug builds: the same note's age only grows, so a younger one here is another note with the listed one's serial.
                    assert(!(c->ageMs >= 0 && r.c.ageMs >= 0 && c->ageMs + 1 < r.c.ageMs) && "age must be a note-on serial unique to each note a slot holds");
                    const uint8_t priority = limits.of(c->context, c->selected).priority;
                    if(!best || (r.tier != best->tier ? r.tier < best->tier : priority != bestPriority ? priority < bestPriority : r.before(*best))) { best = &r; bestPriority = priority; }
                    break; // a context's first still sounding
                }
            }
            return best ? int(best->index) : victim(voices, get, limits, none, only);
        }
        uint32_t write(oscairn_voice_budget_importance_t* out, uint32_t capacity) noexcept
        {
            const uint32_t n = out ? std::min(count, capacity) : 0;
            for(uint32_t i = 0; i < n; ++i) out[i] = lists[i].out;
            truncated = truncated || n < count;
            return n;
        }

    private:
        List* find(uint32_t context) noexcept
        {
            for(uint32_t i = 0; i < count; ++i) if(lists[i].out.context == context) return &lists[i];
            if(count == Contexts) return nullptr;
            auto& l = lists[count++]; l.out = {}; l.out.context = context;
            return &l;
        }
        // Its key as before() compares it, with its level worked out once.
        static Ranked rank(const Candidate& c, size_t index) noexcept
        {
            Ranked r; r.c = c; r.index = index; r.db = levelDb(c.loudness); r.struck = c.struck();
            r.tier = !r.struck && r.db < inaudibleDb ? released : c.held ? keyHeld : c.pedaled ? Tier::pedaled : released;
            r.audibility = r.db - (c.ageMs > 0 ? std::min(c.ageMs / 1000 * agedDbPerSecond, agedSecondsMost * agedDbPerSecond) : 0);
            return r;
        }
        static void insert(List& l, const Ranked& r) noexcept
        {
            uint32_t n = l.out.count;
            if(n == OSCAIRN_VOICE_BUDGET_LEAST && !r.before(l.least[n - 1])) return;
            uint32_t at = n < OSCAIRN_VOICE_BUDGET_LEAST ? n : n - 1;
            while(at > 0 && r.before(l.least[at - 1])) { l.least[at] = l.least[at - 1]; --at; }
            l.least[at] = r;
            if(n < OSCAIRN_VOICE_BUDGET_LEAST) l.out.count = uint16_t(n + 1);
        }
    };

    // Each note's voices and loudness, for notes(): add every sounding voice, then write.
    template<size_t Capacity>
    struct Report
    {
        std::array<oscairn_voice_budget_note_t, Capacity> notes{};
        uint32_t count = 0;
        void add(int32_t noteId, int16_t port, int16_t channel, int16_t key, double loudness) noexcept
        {
            for(uint32_t i = 0; i < count; ++i)
            {
                auto& n = notes[i];
                if(n.note_id == noteId && n.port_index == port && n.channel == channel && n.key == key)
                { ++n.voices; n.loudness = std::max(n.loudness, float(loudness)); return; }
            }
            if(count < Capacity) notes[count++] = {noteId, port, channel, key, 1, float(loudness)};
        }
        uint32_t write(oscairn_voice_budget_note_t* out, uint32_t capacity) const noexcept
        {
            const uint32_t n = out ? std::min(count, capacity) : 0;
            for(uint32_t i = 0; i < n; ++i) out[i] = notes[i];
            return n;
        }
    };
}
