# RFC 0046: Voice importance across parts

Status: accepted by the owner, 2026-11-17 ([his answers](#the-owners-answers)); steps 1 to 5 built
([as built](#as-built-steps-3-to-5)), 6 and 7 to come. Builds on the voice budget ([RFC 0018](0018-voice-budget-and-stealing.md)), sound
contexts and shared instances ([RFC 0005](0005-sound-contexts.md),
[RFC 0021](0021-sounds-performances-and-library.md)) and the load guard, lanes and placement of
[RFC 0045](0045-audio-scheduling-on-small-devices.md) (its no-glitch guard, steps 12a to 12o).

Korg, Roland, Yamaha and Nord products are named here only to compare how instruments share
voices. Oscairn is not affiliated with or endorsed by any of them.

## Progress

Checked when built and tested; the sections below say how.

- [x] 1. `oscairn.voice-budget` draft 2: the importance report (`importance`, a fixed list of a
      context's 16 least important voices), carried in the worker's shared block beside today's
      note report ([the report](#a-what-an-engine-reports)); both CLAP hosts ask for draft 2, then
      draft 1; the worker bounds the lists and names their contexts by the engine's IDs; tested by
      `clap-host-rust` (both hosts' reports alike) and `voice-extension-hostile` (bounded)
- [x] 2. The shared helper in oscairn-clap: one order used both for the report and for
      `victim()`, so an engine gives up exactly what it reported
      ([the scale](#b-one-scale-for-every-engine)): `oscairn::voices::Importance`, `Candidate`'s
      `pedaled` and `ageMs`; the sound-contexts Engine's `importance`, its voices' ages and pedal;
      the Basic Synth (0.3.0) and the voice fixture on draft 2; tested by `voice-importance` and
      `voice-importance-workers` (the list's order, a held key above louder pedaled ones, a key just
      struck above both, and a context lowered by two giving up the two it listed first). The
      host's check that the plugin kept to it is step 3's
- [x] 3. The host's decision in `budget.rs`: one importance order for reported voices and host
      notes; the lane, total, loss and deadline rules take the least important voices from the
      resources each rule constrains; per-context limits from the decision; draft 2's entries carry
      their unit's weight; the engine-core-diff, engine-run-diff and engine-audio-diff vectors
      recorded again ([the decision](#c-the-hosts-decision), [as built](#as-built-steps-3-to-5));
      tested by `engine-core-importance` (the scenario modeled in every placement)
- [x] 4. Thread-bound or device-bound told apart from the worker's wake wait, run and CPU time
      ([which limit](#which-limit-is-binding)); `heldOff` and `mismatch` in `engine.timing`
- [x] 5. Tests: the owner's scenario in every placement mode and in process, through real workers
      (`voice-importance-scenario`); a plugin without the report; no flapping over 2,000 blocks;
      idle cost ([as built](#as-built-steps-3-to-5))
- [ ] 6. The plugins session: Sampler, VA and FM publish through the helper, a release each
- [ ] 7. On the keyboard: the scenario played and measured

## Problem

The owner, 2026-11-17, recorded as given:

- "we really cant have a lead synth dying while keeping pedaled notes that are dying out"
- "the voice weighting is nice inside the engine, is there a way to make that apply between all
  parts?"
- "if a piano part has 30 pedaled notes, and a lead synth plays and its expensive, and dropping
  the oldest/quietest 10 pedaled notes would add headroom for the lead synth, it should do that.
  not sure how the kronos currently handles things like that"
- "although if a lead voice is heavy and needs more time, it shouldnt drop voices on a different
  core, that wouldnt help it right? or would the engines move between cores to make better use of
  the space"
- "check how that would work with workers in separate processes, chain mode, and pool mode"

Standing rules: a performance instrument never glitches (shed voices under load, never drop
audio); a part's last voice is protected; engines stay portable across devices.

## Today

### Inside one engine

An engine with `oscairn.voice-budget` (`own_voices`) chooses its own victims.
`oscairn::voices::victim` (core/clap-ext/include/oscairn-clap/voice-budget-plugin.hpp:74-100)
ranks `Candidate{context, selected, held, ending, mono, loudness, age}` (:60-66): never a held
voice of a mono or legato context, nor a held voice of a context at or under its reserve unless
the new voice is that context's; then released before held, the incoming note's own context
before others, lower part priority first, the quietest, then the oldest. Voices already ending
(a choke's fade) are skipped. The sound-contexts helper fills it
(sound-contexts-plugin.hpp:228-237): `held` is the key (cleared at note-off, :241, :285), so a
note held only by the sustain pedal ranks as released; `loudness` is the engine's own
(`derived().loudness(v)`, e.g. plugins/basic-synth/basic_synth.cpp:134: level × velocity ×
envelope).

### What the extension carries

Host to plugin (core/clap-ext/include/oscairn-clap/voice-budget.h): `set_limits(limit, contexts[])`
between blocks, each context with `limit` (0 none), `reserve`, `priority` (0-10) and the mono and
legato flags (:82-89). Built by `limit_voices` (core/engine/rust/src/run.rs:650-682) from the
part's zone (`reserve`, `priority`), sent only when something changed.

Plugin to host, after each `process()` on the worker's audio thread: `active_voices` (voices
sounding, fading ones included, and the plugin's own cost per voice) and `notes`: every sounding
note with its voice count and loudness (the largest voice gain, envelope × velocity × volume
expression, *not* the patch's output level). The worker copies them into its shared block
(core/plugin-host/src/worker_shared.h:84-87, up to `OSCAIRN_WORKER_MAX_VOICE_NOTES` = 512 notes
of 20 bytes); the clap host hands them on (core/clap-host/rust/src/host.rs:2465-2475). The engine
core writes each note's loudness over the host's slot velocity (notes.rs:402-407,
route.rs:374-384), so the host's "quietest" means the plugin's report where there is one, the
note-on velocity elsewhere. So the host already sees every note of every budgeted instance each
block; what it lacks is voices (a note may be several), the part's gain, and a single order.

### Where time per voice comes from

The host measures it. `Activity::observe` (core/engine/rust/src/budget.rs:76-89): a block with
no notes updates the rest cost; with notes, the time above rest divided among them (rises 0.1,
falls 0.3, a sample capped at four times the estimate). For an `own_voices` plugin it is per
voice (notes.rs:399) and `per_note_scale` turns it per note (:400); the plugin's own `voice_cost`
only seeds the estimate before the first measurement (notes.rs:395-398). A block of a late lane
is paired with the notes live a block earlier (notes.rs:375-378, RFC 0045). The host's estimate
is the trustworthy one: it is measured on this device, under this load; a plugin's estimate is a
guess made without knowing the core it runs on.

### What triggers shedding, and who picks

A **lane** is one audio thread: the steps rendering on one plugin host's thread share one
(core/engine/src/graph.cpp:866-868); an in-process plugin's lane is the engine thread's. Each
block (`Notes::end`, notes.rs:474-500):

| Rule | Trigger | Scope | Who picks | Code |
|---|---|---|---|---|
| Note-on (admit) | lane estimate + the new note past 85 % (`SOFT`) of the lane's limit | the lane | host, `choose_soft` | budget.rs:368-387 |
| Lane over | lane measured past its limit two blocks running | the lane | host, `choose_soft` | budget.rs:415-436 |
| Hard limit | estimate past the time before the lane's block is missed | the lane | host, choke (a click, last resort) | budget.rs:340-359 |
| Lanes' total | sum of lanes past `budget × audio cores` two blocks running | every lane | host: the lane with the **highest cost per note** | budget.rs:440-471 |
| Lost blocks | a lane marked lost in two of its last eight blocks | each marked lane | host `choose`, then `give_up` | budget.rs:491-518 |
| Deadline | callback slack low in two of four blocks, or a block lost anywhere | every lane | host `choose`, then `give_up` | budget.rs:521-525 |
| Voice limit | each block | per instance and per context | the plugin, within the limit | notes.rs:430-445, budget.rs:574-588 |

A lane's limit is 80 % (`LOAD_MARGIN`) of its time: the worker allowance, the engine's deadline
for in-process plugins, the period for a lane a block late, and for an instrument also room for
the chain after it (notes.rs:240-263). The total is `budget × cores`, or `0.8 × period × cores`
(notes.rs:240), the cores being the usable cores less one (graph.cpp:169).

The host's order (`choose_on`, budget.rs:261-309): released (key up, pedal or not) first; then
held keys beyond the part's reserve, the part's highest and lowest held keys last; within each
rank the part of lower priority, a plugin honoring the volume expression, the quietest, then the
oldest. A plugin with the voice budget that keeps within its limit is skipped by the soft rules
(:268): its limit, from the lane's room (budget.rs:578-588), does the work; the loss and deadline
rules lower that limit (`give_up`, budget.rs:475-485, which records `trouble` for the way back)
and fade a note from outside only past `BACKSTOP_BLOCKS` or at a limit of 1.

**The last voice.** `choose_soft` never takes a connection's last note (budget.rs:256-258,
:272); the loss and deadline rules stop at a limit of 1 or one note left (:272) and then count
`unfit` (:511-515). Only the hard choke can take a last note.

**The way back** (`recover`, budget.rs:535-568): a lowered limit returns a voice every 8 healthy
blocks below the ceiling less one, a probe every 750 above it, only where the lane's recent worst
block plus a voice fits; the ceiling forgotten after 10 s healthy.

### Why the owner's lead dies

Three places in the code produce exactly his complaint:

1. **The total picks by cost, not importance.** `device` (budget.rs:455-465) gives up a voice on
   the lane where one voice frees the most time. An expensive lead voice frees more than a cheap
   pedaled piano note, so the lead goes first.
2. **A shared instance's contexts lose in proportion.** `Notes::voice_limit` (notes.rs:441) gives
   each context its share of the instance's cut by its admitted notes, so a lead context sharing
   an instance with a pedaled piano loses voices at its share however unimportant the piano's
   tails are; the engine must then keep each context within its own limit
   (`victim(..., only)`).
3. **Pedaled and released are one rank.** Neither order separates a note the player still holds
   with the pedal from one already in its release, nor protects a note just struck.

Engines without the extension get the host's steal (route.rs:357-362), ordered by note-on
velocity and age; the host knows the pedal (route.rs:276-279) but the order does not use it.

## Design

### Goal

One importance order over every sounding voice of every part. When time must be freed, the least
important voices go first, wherever they are **among the voices whose removal frees the time
that is short**, as many as it takes and no more; never a part's last voice unless nothing else
remains. The bold clause is the honest limit, settled in [2c](#c-the-hosts-decision): which
voices free which time depends on threads and cores.

### (a) What an engine reports

`oscairn.voice-budget` draft 2 adds one call, made where `notes` is made (the worker's audio
thread, after `process()`); the engine thread never calls a plugin. The worker copies the result
into its shared block, as it does the notes; the engine reads it after the block, with the rest.

```c
enum { OSCAIRN_VOICE_BUDGET_LEAST = 16 };

typedef struct oscairn_voice_budget_least
{
    uint8_t  tier;     // 0 released or inaudible, 1 held by a pedal, 2 key held
    uint8_t  flags;    // OSCAIRN_VOICE_BUDGET_STRUCK: struck under 50 ms ago
    uint16_t age_ms;   // since note-on, saturating at 65,535
    float    level_db; // the voice's gain now at the instance's output, dBFS: envelope, velocity,
                       // volume expression and the sound's own output level; -120 for silence
    float    weight;   // its cost against the context's other voices (layers, oscillators); 0: 1
} oscairn_voice_budget_least_t;

typedef struct oscairn_voice_budget_importance
{
    uint32_t context; // a sound context's ID, or OSCAIRN_VOICE_BUDGET_SELECTED
    uint16_t voices;  // its voices sounding now, those already fading out not counted
    uint16_t count;   // entries in least, at most OSCAIRN_VOICE_BUDGET_LEAST
    float    weight;  // the sum of its sounding voices' weights; 0: voices
    oscairn_voice_budget_least_t least[OSCAIRN_VOICE_BUDGET_LEAST]; // ascending importance
} oscairn_voice_budget_importance_t;

// [audio-thread, after process()] one entry per context sounding; returns how many it wrote.
uint32_t (CLAP_ABI* importance)(const clap_plugin_t* plugin, oscairn_voice_budget_importance_t* out, uint32_t capacity);
```

**Its ID.** Draft 2 is `OSCAIRN_EXT_VOICE_BUDGET_DRAFT_2`; `OSCAIRN_EXT_VOICE_BUDGET` keeps meaning
draft 1, so an engine rebuilt against the new header with a four-entry table keeps answering draft
1 and keeps its voice budget (the SDK owner's review: the first patch moved the unsuffixed name to
draft 2, and such an engine then answered draft 2 with a null `importance`). The host asks for draft
2, then draft 1, and treats a draft-1 table or a null `importance` as no report: today's cost-only
behavior, never a refused plugin.

A context's entry is 204 bytes; 16 contexts are 3,264 bytes in the shared block beside the
10,240 the note report already takes.

**A listed voice is the engine's unit of giving up**: the same thing its voice limit and
`active_voices` count, since the host lowers that limit by the entries it takes. The Sampler's unit
is a note (one slot starting up to 16 layers), the VA's and FM's a voice; never a layer of a note
the engine gives up whole. Units differ in cost (a 16-layer piano note and a one-layer one), so
each entry carries a **weight**: its cost against the context's other units, in whatever measure
the engine has (the layers or oscillators it runs; 0 counts as 1), and each context the sum of its
sounding units' weights. The host still measures one cost per voice for the instance (its time
above rest over `active_voices`, which no engine can estimate for the core it runs on); a weight of
1 then costs that times the instance's voices over their total weight, and an entry its weight
times that. An engine whose units cost alike leaves the weights at 1.

**The level is the cheap one, the same in every engine**: `level_db` is 20 log10 of the product of
the voice's envelope level, its velocity gain, its volume expression and the sound's own output
level, 0 dB being its sample or oscillator at nominal full scale; no pass over its samples (a
measured output level would cost one pass per voice per block). The host adds the part's gain.
The tier, the struck flag and the age are carried apart from it. Voices a context may never give up (rule 1 of the
extension: a held mono or legato voice, held voices within the reserve) are not listed.

**Why 16 and not one number.** A limit applies a block after it is sent, and a fade takes one or
two more, so a decision is answered in about three blocks. With one number per engine (its least
important voice) the host learns one victim per instance per decision: ten piano voices take ten
decisions, and with today's two-block hold between give-ups that is some 30 blocks, 80 ms, while
the lead's lane loses blocks. With 16 per context the host chooses all ten in one block and they
are fading within three. A count under fixed thresholds was rejected: it needs the thresholds
agreed in advance and says no more than the list at the same size. Reporting every voice was
rejected: the note report already shows that an instance can hold hundreds, and the decision
needs only the bottom of each list. More than 16 from one context in one decision: the host takes
16 and the next block's report continues the list.

**The cost per voice** is the host's: `note_now() / per_note_scale` for the instance, measured as
today. A plugin's estimate keeps seeding it. One cost for every voice of an instance is a known
simplification (a released Sampler voice streaming from disk costs about what a struck one does;
a VA voice in its release costs what it did held); per-voice costs were rejected for now:
no engine measures them and the host could not check them.

### (b) One scale for every engine

The order must mean the same in the Sampler, VA, FM and a third-party plugin. It is a key in
three parts, compared in order, which the host packs into one float:

1. **Tier.** 0: released (key up, no pedal holding it), or inaudible (its level at the plugin's
   output below -60 dBFS: in a stage mix it is not heard; judged there, not after the part's gain,
   so the plugin's own order and the host's agree); 1: held only by the sustain or sostenuto
   pedal; 2: key held. A voice **just struck** (younger than 50 ms) is never judged inaudible (an
   attack is quiet at first) and goes after every other voice of its tier and part priority: a
   flag within its tier, not a tier above the held keys, so a held key always outranks a note
   struck and let go (the owner's first answer). A held voice of a mono or legato line and held
   voices within the part's reserve are not candidates at all.
2. **Part priority**, the player's: the existing 0-10 `priority` on the part's input, lower giving
   way first (`input.priority` in the Performance document, read at
   core/control/src/performance.cpp:1293-1295, default 10, on the Part page as **Priority** since
   RFC 0018). It is the "protect this part" weight: the lead at 10, the piano at 8. Default
   neutral (every part 10). No new field.
3. **Audibility**: `m = level_db + G_part - 1 dB × min(age in s, 10)`, the voice's level as heard
   in the mix, less a decibel per second of age up to ten (a 10-second-old note counts 10 dB
   quieter than a fresh one of its level).

Packed (budget.rs `key`): `S = 100,000 × tier + 2,000 × priority + 1,000 × struck + clamp(100 + m,
0, 999)`, lower going first; a part's highest and lowest held keys (the bass and the melody of a
chord) add 30,000, last within the held tier.

**Who applies the part's gain.** The engine knows its voice's level including the sound's own
output level; it cannot know the mixer. The host knows the gain from the instrument's output to
the main outputs: the product of connection gains along the path (graph.cpp:874-879 computes the
largest gain from each step to the next; notes.rs:455-465 already walks such a product backward
for losses). `G_part` (dB) is that largest path gain for the context's output, computed when the
plan is compiled, and added by the host to every level of that context. Whether the part's level
fader reaches the core as a connection gain or as a mixer parameter is to be confirmed in step 3;
if a parameter, the control side passes the level with the part's zone, as it passes reserve and
priority. A part at mute counts `G_part` of -120 dB: its voices rank lowest within their tier.

**What changes in `victim()`.** Its order is right in substance and stays the same rule; three
refinements: the pedal tier between released and held; the just-struck tier; the inaudible floor.
The quietest-then-oldest tie-break becomes the audibility term above, with the part's gain
omitted inside the engine (it is the same for every voice of a context; across contexts the
engine has the contexts' priorities but not their gains, so a shared instance's own choice may
differ from the host's where two contexts' part levels differ; the host's per-context limits
then decide which context gives way, and the engine chooses within each). The rule "the incoming
note's own context first" stays for admission only; the importance list describes the shedding
order with no note incoming. Both the report and `victim()` call one key function in the helper,
so an engine gives up what it reported.

**Engines need the pedal.** Tier 1 needs the engine to know a voice is held by the pedal: the
sound-contexts helper sees the sustain controller already (it is how a pedaled note keeps
sounding) and records it on the voice. A third-party plugin implementing draft 2 by hand does the
same.

### (c) The host's decision

Each block, after the measurements (`Notes::end`):

1. **Which time is short, and so which voices count.** Each rule names the voices whose removal
   frees the time it lacks (the next section says how the host tells them apart):
   - **A lane over its own time** (lane over; a lane losing blocks; the hard limit): only voices
     rendered *on that thread*.
   - **The device out of core time** (the lanes' total; the deadline with every lane busy): any
     voice on any lane.
2. **How much.** `T`, in microseconds: for a lane, its measured time scaled as today less the
   soft limit (budget.rs:428-429); for the total, the excess less what is already fading
   (budget.rs:450-454); for losses and the deadline, which say nothing of how much, one voice per
   decision as today (budget.rs:504, 522).
3. **Which.** A merge of the candidate contexts' lists (each entry's `S` with the context's part
   gain and priority applied; its cost the instance's cost per voice), plus the host's own notes
   for engines without the report ([2d](#d-engines-without-the-report)), taken lowest first until
   their costs reach `T`. A context's last voice is skipped while anything else is listed; only
   when nothing else remains does the rule fall through to today's floor (`unfit`, or the hard
   choke where a block would be missed).
4. **Telling the engines.** For each context that gives `n` voices: its per-context limit
   (`oscairn_voice_budget_context.limit`, already in `set_limits`) becomes its listed `voices`
   less `n`, and the instance limit their sum. No new message. The decision replaces the
   proportional split of notes.rs:441 for an instance whose every context reports; draft-1
   plugins keep the split.
5. **Hold and way back.** After a decision for the total, `DEVICE_HOLD_BLOCKS` (8) before the
   next, as today; for a lane, its loss hold. A lowered context limit is an entry's `trouble`
   and comes back by today's `recover` (the learning "a lowered voice limit comes back while
   notes are held": fast to just under where it hurt, then probing, only where the measurements
   say the voice fits). No new hysteresis: the triggers already need two blocks in a row, and the
   way back already waits for measured room.

**The contract, and its check.** Lowered from `voices` to `voices - n`, the engine gives up, at
the start of its next block, the `n` voices it listed first, not others. The helper guarantees it
by keeping the slots it listed, not by working the order out again: between the list (after block
k) and the drop (at block k+1's start) ages, envelope levels, the 50 ms mark and the -60 dBFS floor
move, so the order computed again can differ (the SDK owner's case: B released at -30 dB, 2 s old,
and A released at -40 dB, struck 48 ms ago, list as [B, A]; a block later A is no longer struck and
the order alone takes A). `Importance::next()` gives up the listed voices first, in listed order,
each only while it still sounds in its slot (the same note-on serial, not fading out, not kept by
rule 1 now), and works the order out only beyond them; a listed voice that ended or whose slot a
new note took is skipped, not mis-dropped. The host checks it: on the next report the context's `voices` must be at or
under the limit, and no listed survivor older than a block may score below the highest score the
host chose to drop; a miss is counted (`importance_mismatch` in `engine.timing`, per node) and
fails the tests, and the host goes on (a lying engine costs only its own sound).

#### Which limit is binding

From the code, on Linux (the unit):

- **Policy.** Every audio thread is `SCHED_FIFO`: the engine's at 80, each worker's and the
  graph's helpers' at 70 (core/plugin-host/include/oscairn/realtime.h:18-23;
  core/plugin-host/src/Worker.cpp:150-172).
- **Pinning.** A thread is kept to cores only when `OSCAIRN_AUDIO_THREAD_CORES`, or its side's
  own list, is set (Worker.cpp:160-170, from the profile's `audioThreadCores`,
  `engineAudioCores`, `workerAudioCores`: hosts/standalone/Profile.h:25-35,
  hosts/standalone/Main.cpp:1016-1017). The FANTOM-07 profile sets none of them
  (hosts/standalone/profiles/fantom-07.json:7: `cores` 4, `realtimeCores` 1, `workers` "chain").
  So the engine thread and every worker's audio thread run **unpinned**, placed by the kernel's
  real-time scheduler, which pushes a runnable FIFO thread to an idle core every time it wakes.
  The image's isolated partition (`realtimeCores`, os/scripts/audio-cores.py, RFC 0035, verified
  only in QEMU) is not what the unit runs: the measured builds were started from /home/root
  (notes w27). No `isolcpus`. The engine counts `usable cores - 1` (3 on the unit) as the audio
  cores for the total (graph.cpp:169).

So the model is **a shared pool of core time**, not per-core budgets: dropping a voice anywhere
shortens the work every core has to share. What nothing else can fix is **one thread needing more
than its own time**: a worker's audio thread renders its plugins one after another on one core
and cannot use a second, and its block must be done within its allowance (half a block at 128
frames, 1,333 µs, or the period for a lane a block late). On the unit, ST Final under chain
placement is 5 workers, Sick tines 7, TSO Carol Bells 10 (notes w27, main-2026-11-17), on 3 audio
cores, with workers at rest at 40-80 µs a block and ST Final's busiest in a take at 476 µs (Bass),
257 (Arp) and 179 (Arp inserts) (day-2026-11-16): the cores are mostly free, and a part's own
thread is the constraint far more often than the device.

**Telling them apart from what is measured.** Each worker reports, per block
(worker_shared.h:89-95; recorded at core/plugin-host/src/WorkerHost.cpp:1170-1174): `wokeAt -
sent` (the time from the engine's request until the worker's thread ran: the wait for a core),
`doneAt - wokeAt` (its run, wall clock) and `cpuNs` (CPU time the thread used in that run). The
budget sees only `lastMicroseconds` today (WorkerHost.cpp:1042). Step 4 hands the core the
other two:

- **The lane's own thread is over**: its run (or CPU time) past the lane's limit with a short wake
  wait. Nothing outside the thread helps; only voices on that thread are candidates.
- **The device is out of core time**: wake waits growing on several lanes, or run time well above
  CPU time (the thread was preempted), with the lanes' total past `budget × cores`. Any voice
  helps; the global ranking applies.

**Moving a worker to make room.** The kernel already moves unpinned workers between cores at
every wake; there is nothing to gain from re-pinning live, and pinning a thread to a core would
turn the shared pool into per-core budgets, which is worse. Moving a *plugin* to another worker
process means loading a new instance there: seconds of loading, its memory twice for a while, and
a cold first block; that is placement at Performance load (pool mode does it by measured cost,
WorkerHost.cpp:2244-2267), and splitting a shared instance under load is RFC 0045's step 9, not
built. **Recommendation: no live moves.** Placement stays a load-time decision.

#### Placement modes

How plugins map to threads is `compute.workers` / `OSCAIRN_WORKERS` (core/plugin-host/src/
plugin_host.cpp:331-336): `plugin`, `chain` or `pool`.

**plugin** (a worker process per plugin instance; the default off the unit).
1. A lane is one plugin's thread; its limit the worker allowance (or the period when a block
   late). Only that plugin runs in it.
2. Voices sharing the lead's thread: the lead's own (and any other part's sharing the lead's
   *instance*, a shared Sampler).
3. Every other part's voices free only device time.
4. Lists: per instance; per context where one instance serves several parts (separate Sampler
   instances become the default with the branch sharing-default; a shared instance stays as the
   memory fallback and reports a list per context).
5. Lead's lane over: the lead's engine sheds by its own order (its released tails and oldest,
   then quieter held notes), never its last voice. The global ranking acts on the total and the
   deadline.
6. Moving: no.

**chain** (the unit's: a part's instrument and its inserts in one worker, rendered one after
another in one round trip; groups from the audio connections,
core/plugin-host/src/plugin_host.cpp:349-370: an instrument heads a group, an effect whose audio
all comes from one group joins it).
1. A lane is a part's chain; its limit as above, the instrument's lowered to leave room for the
   chain after it (notes.rs:260-263, RFC 0045 12c).
2. Sharing the lead's thread: the lead's instrument; its effects too, but effects are not voices
   (their cost falls only with the instrument's output going quiet, if they sleep).
3. Other parts' voices free device time only.
4. Lists: per instance, per context.
5. As plugin mode. 6. Moving: no.

**pool** (several worker processes, 3 by default, each instance placed at load in the one that
costs least by its plugins' measured costs, then fewest nodes, WorkerHost.cpp:2179-2267; a group
marked alone gets an empty host where one is free).
1. A lane is one pool process's thread, running every plugin placed there in turn; its limit the
   allowance.
2. Sharing the lead's thread: every instrument placed in the same process, of any part: their
   voices free the lead's time directly.
3. Instruments in the other processes free device time only.
4. Lists: per instance, per context; several instances on one lane all join the lane's merge.
5. Lead's lane over: the merge takes the least important voices *on that lane*, so a pedaled
   piano in the same process gives way before the lead (the owner's example, exactly); only
   then the lead's own. The global ranking acts on the total.
6. Moving: pool re-places only at load. Placing by importance (keeping a part marked priority 10
   away from heavy pedaled parts) could help; it was ruled out by the learning that pool
   placement scattered chains (0 of 22 nodes grouped with three hosts), and is not proposed.

**In process** (no worker: `--hosts` tests, desktop builds with plugins in the engine).
One lane, the engine's thread, limit 80 % of the deadline. Every voice shares it: the merge over
the lane is the global merge, and the owner's example works fully.

| Mode | Shares the lead's thread | The global ranking can free | Only the lead's engine can fix |
|---|---|---|---|
| plugin | the lead's instance (all its contexts) | device time: any part's voices | the lead's thread over its allowance |
| chain | the lead's instrument (its inserts are not voices) | device time: any part's voices | the lead's chain over its allowance |
| pool | every instrument placed in the lead's process | that process's time, then device time | the lead's process over its allowance with nothing else in it to drop |
| in process | everything | everything | nothing beyond the last voices |

- **plugin:** the example works only when the device is out of core time (all three audio cores
  busy); when the lead's own worker is over, the lead sheds its own least important voices.
- **chain:** the same as plugin; on the unit today (5 to 10 workers, 3 cores, mostly idle) the
  lead's own worker is the likely limit, so dropping piano notes would usually not help and is
  not done.
- **pool:** it works whenever the piano and the lead share a process, and otherwise as plugin.
- **in process:** it always works.

**The owner's question, answered.** A lead voice that needs more time on its own core is not
helped by dropping notes on other cores, so in that case the lead sheds its own least important
voices instead. The workers are not pinned: Linux moves them between the free cores every block
by itself, so dropping voices elsewhere helps only when all the cores are busy at once, and that
is when the global ranking takes the pedaled piano notes first.

### (d) Engines without the report

Draft-1 plugins and plugins without the extension are ranked by what the host knows: tier from its
own state (released, held by the pedal: route.rs:276-279's test, key held), just struck from a
note-on block stamp added to the slot (4 bytes), level unknown and taken as the middle of the
range, `-24 dBFS + 20 log10(velocity)` (or the draft-1 note loudness where reported), plus the
part's gain; age from the stamp. Their cost per note is the host's, as today. The host's backstop
steal for them follows the same key, so its order becomes released, then pedaled, then held, by
audibility and age, instead of today's velocity-then-oldest. The change is small: one stamp per
slot and the key in `choose_on`.

### (e) Cost on the audio thread

No locks, no allocation, no system calls on either side. In the engine: a fixed array per
instance's contexts (16 × 136 bytes) in the plan, sized at compile; per block, the copy (already
done for the note report). The merge runs only when a rule fires: at most a few dozen contexts of
16 entries, taken by repeated minimum over the list heads, bounded by the voices taken (at most 16
per context per decision). Idle: nothing but the copy. In the engine plugin, the helper keeps the
16 lowest keys over its voices after `process()`: one pass, an insertion into a 16-slot array per
voice that beats the 16th; about 128 × a few comparisons for a full Sampler, estimated at a few
microseconds on an A53 (unverified; step 7 measures it). Memory: 2.2 KB per worker in its shared
block, about the same per instance in the plan.

## How other instruments do it

Only what the sources found say; quoted sparingly.

- **Korg KRONOS.** The Operation Guide's introduction says its polyphony "varies depending on
  which synth engines are being used", that it "automatically divides its processing power
  appropriately", and that it "monitors the overall processing power, reducing the overall number
  of voices if necessary" (KRONOS Operation Guide, "Introduction to KRONOS", manualslib page 24).
  No documented order for which voices go across programs or Combination timbres was found; it
  could not be established here.
- **Roland.** The Fantom-S Owner's Manual ("About simultaneous polyphony", p. 19): Voice Reserve
  "lets you reserve a minimum number of notes that will always be available for each part", the
  total of the reserves held to the polyphony (64); each patch's Voice Priority, LAST (the oldest
  goes) or LOUDEST (the quietest goes). Older Fantom manuals only; the current FANTOM's reference
  manual text was not found.
- **Yamaha MODX.** Not established from Yamaha's documents: the specification page refused the
  fetch and the reference manual PDF could not be read here. The separate AWM2 and FM-X pools are
  widely reported but unverified; the owner's MODX is the place to measure (it is the comparison
  he set for new engines).
- **Nord.** Nord's Stage 4 specifications, as a search summarized them (not fetched), list the
  synth section's polyphony on its own (46 voices), the piano section's elsewhere (120 by a
  retailer's listing): separate pools per section, so a pedaled piano cannot take a synth voice.

Worth copying: Roland's per-part reserve and loudest-first priority, which Oscairn already has
(the part's `reserve` and `priority` since RFC 0018, loudness in both orders); the Kronos's
device-wide reduction by processing power, which is the lanes' total rule, now ordered by
importance. Nord's separate pools are what plugin and chain placement already are per thread:
the piano cannot take the lead's thread time, nor give it any.

## Plan

In order; each step proves something before the next.

1. **Extension and transport** (host). `voice-budget.h` draft 2 (the struct and call above; the
   host asks for draft 2, then draft 1); the worker's shared block gains the importance array,
   `MADE_FROM` and bindings updated by hand (the bindgen learning); the clap host hands it on
   beside `voice_report`. Proves: a test plugin's list reaches the engine core intact, in a
   worker and in process. Pre-release: no compatibility beyond the draft-1 fallback.
2. **The helper** (host, core/clap-ext). One key function; `importance()` and `victim()` on it;
   the pedal tier in the sound-contexts helper; a validator check that a plugin lowered by `n`
   gives up its first `n`. Proves: report and action agree, in voice_budget.cpp's plugin.
3. **The decision** (host, core/engine/rust/src/budget.rs and notes.rs). The key in `choose_on`
   for host notes; the merge for reported contexts; `device` by importance; per-context limits
   from the decision; `G_part` at plan compile; `importance_mismatch`. The engine-core-diff and
   engine-run-diff vectors are recorded again from the Rust core: the order of victims changes on
   purpose (as at 35b7b776); engine-audio-diff should replay unchanged, and a change there is a
   finding.
4. **Which limit** (host). The worker's wake wait and run/CPU time into the core's `Rendered`;
   lane rules restricted to their thread, the global merge to the device rules; in
   `engine.timing`.
5. **Tests** (host), with `OSCAIRN_AUDIO_CORES=1` to force the total where needed:
   - the owner's scenario: 30 pedaled piano notes plus an expensive lead under overload, in
     **plugin**, **chain**, **pool** with both in one process, **pool** with them apart, and
     **in process**. Where they share the thread, or the total is forced: the lead keeps every
     voice, the ten oldest and quietest piano notes go, and no more than the excess needs. Where
     the lead's own thread is over: the lead sheds its own released and oldest voices, the piano
     loses nothing. No part silent in any case;
   - a third-party-style plugin without the report beside one with it: the host's notes ranked by
     tier and age (pedaled before held);
   - no flapping: an overload held steady for 10 s gives a stable limit, voices back only as
     `recover` allows;
   - idle cost: a full report at rest adds no measurable block time against the same run without
     it.
6. **Engines** (the plugins session): Sampler, VA and FM publish through the helper and record
   the pedal on their voices; a release each, with the plugin version bumped.
7. **On the keyboard** (host, with the owner): Sick tines' pedaled piano plus a VA lead in chain
   placement: lost blocks, which notes were given up (by tier), `importance_mismatch`, and the
   helper's cost per block on the A53; then the same in pool.

Risks: the 50 ms just-struck window may protect fast repeated notes too long (counted per voice,
so the key drops after 50 ms); `G_part` from the path gain may disagree with what is heard where
an effect changes the level a lot (a compressor), accepted; one cost per voice per instance
misprices voices of very different patches in one shared instance; a third-party plugin
reporting a flattering list costs only its own sound, and the check makes it visible.

## The owner's answers

The owner, 2026-11-17, to the four questions this RFC asked, and his rule over all of it:

1. A key held always outranks a pedaled note, however much louder: **yes** (the -60 dBFS floor
   still takes inaudible held notes first).
2. The existing part **Priority** (0-10) is the "protect this part" control: **priority**, no new
   switch.
3. 50 ms protects a just-struck note: **yes**.
4. The lead may take an extra block of latency before shedding its own voices: **no**.

His rule, one in every placement mode: a less important voice is taken only where taking it frees
time the important voice actually lacks. (a) Voices in the same thread as the lane that is short
always qualify; (b) voices in other threads qualify only when core time as a whole ran out, and then
only as many as free what is missing; (c) never a voice whose removal frees nothing that is short.
A part's last voice is protected. His scenario: 30 pedaled piano notes and an expensive lead under
overload: the oldest and quietest piano notes go, as many as needed, the lead survives, no part
silent.

## As built (steps 3 to 5)

The rule's three cases, in core/engine/rust/src/budget.rs:

- **(a) The lane that is short gives up its own thread's voices**: admission (`admit`), a lane over
  its limit two blocks running (`measured`) and a lane losing blocks (`lost`) choose with
  `Some(lane)`: the least important voice rendered on that thread, a listed one (its context's
  next, `listed_key`) or a host note (`note_key`), on one key (`choose_on`). In a pool process or
  a shared instance that is the neighbors' pedaled tails before the lead's voices; in a worker or a
  chain of its own, the lead's own released and oldest voices.
- **(b) Other threads only when core time ran out, as many as free what is missing**: `device`
  fires on the lanes' total past `budget × cores`, or on a lane held off its core
  (`Lane::judge_held`: over its time, or losing a block of its own, while its threads' CPU time
  fits its limit, with its time well above that CPU time or a wake wait past a quarter of its
  time); it takes the least important voices anywhere until their costs (`victim_cost`: an
  entry's weight at the instance's measured cost) cover the excess, the total's or the held
  lanes'. A held lane's losses take from any lane (`lost`); its own over-time takes nothing from it
  (`measured`). The deadline rule takes from any lane only with the device short, else from the
  lane furthest over and those feeding it (`deadline`).
- **(c) Never a voice that frees nothing short**: a lane rule never looks past its lane; a lane
  marked only for a loss after it is not held (its voices free the lost block's time); and the
  merge stops at a context whose 16 listed voices are spent while more sound unlisted (`unlisted`
  in `choose_on`): those may be less important than anything else, so nothing above its last
  listed one goes until its next report lists them.

What it took besides the order:

- **The report reached the core empty** in every graph: `Plugins::importance` (core/engine/rust/
  src/lib.rs) read `lists` as an argument before the call that set it. The modeled test drives the
  core directly and the step-2 tests ask the host, so neither saw it; `voice-importance-scenario`
  found it. The call now comes first.
- **A listing plugin's limit leaves room for the note just admitted** (the step-3 fault: with the
  piano and the lead in one pool worker the lane over its time took 1 piano and 1 lead voice, not
  about 16 piano). The caps were set to the voices reported less those taken, so the lead's second
  key arrived over its cap and the lead gave up its own held key. A listing plugin's instance limit
  is never cut below what it sounds and its notes; its context's cap counts the voices admitted
  since its report (`Heard::admitted`), and admission makes room for its new note at one voice's
  cost.
- **A lane is judged again only once the voices taken have left its time** (`Lane::settle`, three
  blocks): a listing plugin drops them at its next block and its report at once, while the lane's
  time, the median of five blocks, follows in three. Without it the rule took again on the stale
  median (25 piano voices for an excess of 15).

A listing plugin's context keeps to the cap the budget set on it (`context_cap`), in place of the
proportional split. `importance_mismatch` counts a context found over its cap two reports on, in
`engine.timing` as `mismatch` (expected 0), beside `heldOff`. The vectors of engine-core-diff,
engine-run-diff and engine-audio-diff were recorded again from the Rust core
([developer guide](../developer-guide.md#the-engine-cores-vectors)): the first two because the
victims change on purpose, the third only for the two counters a node's activity gained (its
samples replay unchanged against the old vectors with the activity's digest cut to its old size).

Tests: `engine-core-importance` models the scenario in every placement: pool with both in one
worker, 16 piano voices for an excess of 15, the lead none; apart with the device short, 13 piano,
the lead none; apart with the lead's own lane over, the lead's 3 released, the piano none; one
shared instance, 10 (its one cost per voice prices the piano against the lead); in process, 16; a
piano without the report, 24, the lead none; 2,000 blocks with a new note every 25, no flapping and
no mismatch. `voice-importance-scenario` plays it through real workers in plugin, chain (an insert
after the lead), pool with one process and with two, and in process, a piano without the report,
and 2,000 blocks with new notes: the lead keeps both keys wherever the piano's voices free what is
short, the piano loses nothing for the lead's own lane, no part goes silent.

Through real workers (the fixture, a piano voice 15 µs, a lead voice 180, a 1,000 µs allowance;
Mac, no block lost in any case):

| Placement | Case | Piano voices given up (of 30) | Lead given up, keys kept | A part silent |
|---|---|---|---|---|
| pool, one process | same thread over | 7 | 0, 2 | no |
| in process | same thread over | 12 | 0, 2 | no |
| plugin / chain / pool, two processes | device out of core time | 8 / 9 / 8 | 0, 2 | no |
| plugin / chain / pool, two processes | lead's own lane over | 0 | 2 of its 3 released, 2 | no |
| plugin | piano without the report | 8 (host notes) | 0, 2 | no |
| pool, one process | 2,000 blocks, a new note every 25 | 81 (9 + one per new note) | 0, 2 | no |

Cost: the helper's report for 128 voices (list and write) takes 1.1 µs a block on the Mac (5.5 µs
before: a log10 in every comparison, now one per voice); the A53's is step 7's to measure. At rest
the worker bench (oscairn-handoff-bench, the Basic Synth, one worker, three runs each, alternating
with main at b7a5e3fc) shows no change: median overhead 7.1, 15.0, 16.5 µs before, 6.8, 15.7, 12.2
after, within the machine's noise.

## Ruled out

- One importance number per engine: one voice per decision, some 80 ms for ten notes.
- Every voice reported every block: the decision needs only the bottom of each list.
- Cost-efficiency ordering (importance per microsecond): the owner wants the least important
  first, as many as it takes, not the cheapest decision.
- Re-pinning or moving workers live to make room: the kernel already moves unpinned workers each
  block; moving a plugin is a reload.
- Placement by importance in pool mode: pool placement by cost already scattered chains.
