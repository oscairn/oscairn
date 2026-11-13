# RFC 0045: Audio scheduling and placement on small devices

Status: proposed, 2026-11-13, from the FANTOM-07 measurements of that week and the owner's
decisions of the same day ([below](#the-owners-wishes)). Steps 1 to 3 are built (measurement
and an opt-in experiment); step 4, the miss policy, is built and changes what a missed block
does by default. Builds on plugin worker isolation
([RFC 0010](0010-plugin-worker-isolation.md)), parallel graph rendering
([RFC 0034](0034-parallel-graph-rendering.md)), resource budgets
([RFC 0025](0025-resource-budgets.md)), reliability levels
([RFC 0027](0027-reliability-levels.md)), sound contexts and shared instances
([RFC 0005](0005-sound-contexts.md), [RFC 0021](0021-sounds-performances-and-library.md)),
the voice budget ([RFC 0018](0018-voice-budget-and-stealing.md)), device profiles
([RFC 0024](0024-device-profiles.md)) and the system base
([RFC 0035](0035-system-base.md)).

## Progress

Checked when built and tested; the sections below say how.

- [x] 1. The worker allowance as one named quantity with its derivation in one place
      (`workerAllowance`, `realtime.h`), a supported setting (`OSCAIRN_WORKER_BUDGET_US`, the
      profile's `compute.workerAllowance`); default unchanged ([status](#step-1-status))
- [x] 2. Blocks past the allowance counted apart: played late and silenced, in `session.status`,
      `engine.timing` and `oscairn-engine-timing` ([status](#step-2-status))
- [x] 3. The extra block, experimental: a listed plugin's worker one block late, opt-in per
      plugin ID, fixed at load (`OSCAIRN_WORKER_EXTRA_BLOCK`) ([status](#step-3-status))
- [x] 4. A missed block no longer ends the plugin's notes: no panic for a block the worker was
      handed; the events of a block not handed carried to the next; a short fade instead of a
      cut ([miss policy](#the-miss-policy), [status](#step-4-status))
- [ ] 5. "Too slow" no longer restarts a worker that is making progress
      ([when a restart is right](#when-a-restart-is-right))
- [ ] 6. The allowance from the block's deadline and the graph's waves
      ([the allowance it should be](#the-allowance-it-should-be))
- [ ] 7. One profile field for the audio cores; the display and the app off them
      ([core placement](#core-placement))
- [ ] 8. The extra block entered and left under measured load, with hysteresis, per instance
      ([entering and leaving](#entering-and-leaving))
- [ ] 9a. Fair voices between the parts of a shared instance: a share per context even without a
      CPU budget ([fair voices](#fair-voices-between-the-parts-of-one-instance))
- [ ] 9. Share by default, split under load ([sharing](#share-by-default-split-under-load))
- [ ] 10. Readiness: a Sound not playable until prepared and warmed up, shown as loading
      ([first-use stalls](#start-up-and-first-use-stalls))
- [ ] 11. The timing report: each part's latency state, misses by cause, panics caused by misses
      ([what the tools report](#what-the-timing-tools-must-report))

## Problem

On the owner's FANTOM-07 (four Cortex-A53 at 1.8 GHz, 1 GB, Linux 4.19 PREEMPT without the
real-time patch, 48 kHz, 128-frame blocks of 2.667 ms, performance governor), measured the week
of 2026-11-09:

- **The kernel is good enough.** `cyclictest` at FIFO 80 with a 2,667 µs interval for 30 s,
  engine stopped: worst latency 48 µs on core 0, 38 µs on core 1, average 6 µs. Every device
  interrupt is handled on core 0. Real-time priorities are granted (engine FIFO 80, helpers and
  workers' audio threads FIFO 70). Nothing was pinned.
- **The engine alone is comfortable.** Default graph (sequencer, synth, delay, each in its own
  worker), 15,000 blocks: callback p50 about 0.5 ms, max 1.4 ms; lateness max 0.36 ms; worker
  round trips 64–256 µs, max 0.52 ms; no misses. Load 0.13–0.23 at rest; a ramp of the built-in
  synth: 8 notes 0.28, 16 0.35, 32 0.53, 64 0.81.
- **The app drawing breaks it.** With the app drawing on the instrument's screen (software
  rendering under Weston and Xwayland): callback p50 about 1 ms, max 6.3 ms; lateness max
  3.75 ms; 2 misses in 40 s, the engine and two workers all stalled at the same instant for
  3–6 ms. The owner heard glitches.
- **Real instruments, one part each.** FM: clean to its 16-note cap at 0.23, about 18 µs of
  worker block per note. The Sampler: clean to 32 notes (worker block 936 µs at 8, 1,444 at 16,
  2,438 at 32, 2,855 at 48 where it failed on misses), and a stall on the first 8-note chord of a
  sample set not played yet. Analog (Surge-based): failed at one note, typical block 750 µs with
  stalls of 7–16 ms.
- **Sharing concentrates the work.** Several parts sharing one instance (RFC 0005, RFC 0021) are
  one worker, one core, one voice pool and one time allowance; the other cores idle.

Several Sampler blocks ran longer than the worker's allowance (1.33 ms) while the device never
underran: the rule that judged them is stricter than the device needs, and what it does on a miss
is worse than it needs to be ([below](#what-a-miss-does-today)).

## The owner's wishes

2026-11-13, recorded as given:

- Yes to reconsidering the half-block allowance per plugin; larger buffers for heavy
  instruments; keeping audio off the core the display uses.
- On buffer size: "i can hear the difference between 128 and 256 so idk if theres a way to
  automatically switch back and forth depending on performance."
- On sharing: "share by default split under load sounds great along with those other things":
  the device profile sets the bias between sharing (memory) and separate instances (CPU); voice
  limits follow the parts, not the instance, with the host's voice budget the real limiter; find
  out whether two Sampler instances can share loaded samples.

## The per-plugin time allowance

### The allowance today

`workerAllowance` (`core/plugin-host/include/oscairn/realtime.h`, built in step 1; before it the
same arithmetic sat in `Process::setRate`, `core/plugin-host/src/WorkerHost.cpp`):

- **The quantity.** When the engine hands a worker a block (`WorkerHost::start`) it records the
  time; when it comes for the result (`finish`) it waits at most the allowance from that hand-over.
  The allowance is half the block's period: 1,333 µs at 128 frames and 48 kHz. Offline rendering
  waits 10 s instead.
- **Why half.** RFC 0010 does not say; the code's reason, now written beside it, is that the
  graph's usual longest chain is two workers one after the other (an instrument, then its effect),
  and two whole allowances fit in a block. Since RFC 0034 a worker's wait counts from its own
  hand-over within its wave, so a chain of three workers (the default graph's sequencer, synth and
  delay are three waves) can in the worst case wait 1.5 periods: the rule does not actually
  guarantee the block, it relies on most workers being fast.
- **The override.** `OSCAIRN_WORKER_BUDGET_US` (whole microseconds above 0, up to 60 s), or the
  profile's `compute.workerAllowance`, replaces it. The tests use 50 ms. Nothing else changes it.
- **Too slow.** `missLimit` is a quarter second of blocks in a row, at least 8: 94 at 128 frames.
  A block counts toward it when it is late or silent (below). The supervisor (every 10 ms) then
  fails the worker and restarts it, at most 3 times a minute. A block left unanswered for 250 ms
  is a hang. Both verified in `WorkerHost.cpp` (`setRate`, `missed`, `trouble`, `supervise`).

### What a miss does today

Until step 4 (below: [step 4 status](#step-4-status) says what a miss does now). Read in `WorkerHost::start`, `WorkerHost::finish` and `Graph::process`'s `finish`
(`core/engine/src/graph.cpp`, the `if(!processed)step.host->panic();` line):

1. **A block not finished within the allowance is silenced**, and the worker keeps rendering it;
   its sound is thrown away when it does finish.
2. **The next block is silenced at once if the worker is still busy** with the last one, and is
   never handed to the worker: its notes and parameter events are lost.
3. **Either way the host reports the block as not processed, and the graph then panics the
   plugin and clears the part's note routes.** Every note the part holds ends. One missed block,
   whose own audio would have been a 2.7 ms dropout, becomes every held note of that instrument
   cut. This is likely a large share of what the owner heard as glitches with the Sampler; it is
   unverified on the device (step 2's counts and step 11's report would show it).
4. A block found finished only after the allowance (the engine's own thread woke late, as when
   the display stalled everything for 3–6 ms) is played, and counted toward "too slow" all the
   same.

### Late and silent, counted apart (step 2)

`blockOutcome` (`realtime.h`) names the three outcomes the engine sees: **on time** (done when the
engine came for it, or found done within the allowance), **late** (found done past the allowance:
played) and **silent** (not done within the allowance, or not handed because the worker was still
busy: silenced, the dropout heard). `session.status` gives `lateBlocks` and `silentBlocks` and each
worker's `allowance`, `late` and `silent`; `engine.timing` gives each worker's since the last
reset; `deadlineMisses` keeps its old sum, which also holds a plugin's own block longer than the
period (measured inside the worker), and so can count one block twice.

### The allowance it should be

The allowance protects one thing: the device's deadline. What any worker may take depends on
what must still happen after it in the same block. Proposed:

- **A deadline per block,** not a share per plugin: the callback's start plus the period, less a
  margin for what the engine does after the last wave (mixing, taps, meters, the recorder's copy;
  measured per block, not yet on the FANTOM-07; and the reliability level's margin, RFC 0027).
- **Each worker waits until the deadline less the measured work of the waves after it on its
  path.** The plan already knows each step's wave and successors (RFC 0034); the engine already
  measures each step's cost (`Plan::Step::cost` for local plugins, round trips for workers). A
  worker whose output goes straight to the mix (an instrument with no effect after it, or the
  last effect) may use nearly the whole block; an instrument with a heavy effect after it waits
  less, by that effect's p99.
- **Parallel parts do not share an allowance:** they wait at the same time, so each may use the
  time to the deadline. Only serial chains divide it.
- With the Sampler's measured blocks, this alone would have made 1.4–2.4 ms blocks on time where
  it plays straight to the mix (2.667 ms less the engine's own work after it), without any
  latency (an estimate from the figures above, not measured).

This is step 6. It needs the engine to know the callback's start inside the graph (the host
already gives it to the session for `engine.timing`, `Session::blockTimes`), and it changes what
counts as late, so it goes behind its own setting first and becomes the default only after a
device measurement shows fewer silent blocks and no new underruns.

### The miss policy

In order of what to build (step 4):

1. **No panic for a block the worker was handed.** The worker has its events and will render
   them; a missed block's notes are not lost, only its audio. Report it as processed-but-silent
   (a new host result, or the graph telling a worker miss from a control silence), so the graph
   keeps the part's notes and routes. Panic only where events were really lost.
2. **Carry the events of a block not handed** (the worker still busy) into the next block handed,
   at time 0 and in order, instead of dropping them; then even that case needs no panic. Bounded
   by the event queue; on overflow, the panic as today.
3. **A short fade instead of a cut:** the block before a miss is already played, so the silent
   block starts with the last sample of the last good block faded to zero over about 1 ms, and the
   next good block fades in over 1 ms (the restart's 10 ms fade-in is already there). Repeating
   the last block with decay was considered and ruled out: on a pitched sound it is an audible
   buzz at the block rate, worse than a short gap.
4. **Late delivery** of a missed block, played in the next block instead of thrown away, is the
   extra block entered for one block; it shifts the part by a block from then on, so it belongs
   to [entering and leaving](#entering-and-leaving), not to the miss policy.

### When a restart is right

A restart loses the plugin's voices and costs 100 ms to seconds of silence; it fixes a plugin that
is stuck, not one that is slow. Proposed (step 5):

- **Restart on a crash, a hang (a block unanswered for 250 ms), or memory over its limit,** as now.
- **"Too slow" restarts only when the worker is not making progress:** its blocks-done counter
  (`Shared::blocks`) has not moved for the quarter second. A worker that finishes every block but
  late is overloaded: the host sheds load instead (the voice budget's limit lowered for that
  instance, RFC 0018; then the extra block, step 8), reports it, and never restarts it in a loop.

## Latency that adapts per part

The owner hears 128 against 256 frames, so the device stays at 128 and only heavy parts should
pay.

### One block late for one part

**The extra block** (step 3, built as an experiment): an instance in this state hands its worker
block N and plays block N−1, which the worker rendered during the last period. That part sounds
exactly one block late (2.67 ms at 128 frames); other parts are unaffected; the worker gets its
allowance plus a whole period for each block (4 ms instead of 1.33 at 128 frames), which covers
the Sampler's measured 2.4 ms at 32 notes and Analog's typical 750 µs, though not Analog's
7–16 ms stalls ([first-use stalls](#start-up-and-first-use-stalls)).

What it does to the rest:

- **Note timing between parts:** the delayed part is 2.67 ms behind the others. Layered parts
  playing the same notes flam by that much; for unlike sounds it is below what players notice
  (it is the latency difference between 128 and 256 frames for that part alone), for two
  near-identical layers it can comb. A layer group should enter the state together.
- **Effects:** effects after the part in its own chain follow it a block late, with nothing to
  do. Sends to shared buses (a Performance's reverb) carry the delayed sound into an undelayed
  bus, which is right: the send is late exactly as the dry part is.
- **Transport-following plugins** (an arpeggiator, a sequencer, tempo-synced LFOs) see block N's
  transport and are heard at N+1: a block behind the others. Note effects before the instrument
  are not delayed; only what the delayed instance renders is.
- **Recording:** the recorder copies taps at the end of each block (RFC 0020), so a part's tap and
  the main mix hold the part a block late, as heard. A multitrack recording of a part tap could be
  shifted back by the recorder (it would know each part's state); the main mix cannot be
  un-mixed and records what was heard, which is what the musician played to. MIDI is recorded as
  played. Proposed: shift part taps, not the mix, when step 8 makes the state automatic.
- **Plugin delay compensation** does not exist in the graph today (a host only reports
  `clap.latency`), so nothing has to be told; when it is added, the extra block is one block of
  latency for that node.

### Entering and leaving

Step 8, after the device has measured step 3:

- **The signal:** each worker's block time inside the worker (`Statistics::lastMicroseconds`, its
  distribution in `engine.timing`) against its allowance, over a window: enter when the p99 of
  the last second passes 80% of the allowance or any block is silent; leave when the p99 of the
  last ten seconds is below 40%. The gap between the two is the hysteresis, and leaving waits at
  least ten seconds after entering.
- **Entering** needs a block of sound that does not exist: the part repeats nothing; it plays one
  block of silence (or the 1 ms fade of the miss policy) and from then on is a block late. Best
  done when it misses anyway (the late delivery above): the dropout that would have happened
  becomes the entry.
- **Leaving** throws away one block: done only while the part is silent (the plugin reports sleep,
  or no voices for a second), so nothing is heard. A part that never rests stays late until it
  does.
- **Per instance**, as the state lives in its worker; parts sharing an instance enter together.

### Against switching the device buffer, and against a fixed setting

- **Switching the device's block** (128 to 256 and back) at run time is a device reopen: ALSA's
  period change needs the stream stopped; a gap of tens to hundreds of milliseconds, every part
  pays the latency, and every plugin is activated again at the new size. Ruled out as an
  automatic action; it stays a Settings choice.
- **A fixed profile setting** (a heavy plugin always a block late on this device) is simple and
  predictable, and a fine first use of step 3, but it makes the part pay all the time, including
  at rest.

Recommended: **the per-instance extra block, entered and left under measured load**, with a
profile list of plugins that start in it on a given device as the fallback while step 8 is not
built.

### Step 1 status

Built 2026-11-13. `workerAllowance(frames, rate, override)` in `realtime.h` and `Worker.cpp`
holds the derivation and parses the override; `Process::setRate` calls it. `OSCAIRN_WORKER_BUDGET_US`
is documented in the developer guide as a supported setting; the profile's `compute.workerAllowance`
(0 to 1,000,000 µs) reaches it from the app (`hosts/standalone/Main.cpp`, unless the session set
it) and from `os/scripts/engine-args.py` for the headless session, which exports it. A malformed
value used to become 1 µs (`atol`); it now leaves the derived allowance. Tests: `control-session`
(the derivation at three block sizes, the override, malformed and out-of-range values),
`desktop-profile` (the field, refusals), `session-headless-engine-args`.

### Step 2 status

Built 2026-11-13. `blockOutcome` in `realtime.h`; `WorkerHost` counts `late_` and `silent_`
(a block not handed because the worker was busy is silent); `kick::clap::Statistics` carries
`lateBlocks`, `silentBlocks` and `allowanceMicroseconds`; the session sums them into
`session.status` and gives each worker's in `workers`; `engine.timing` gives each worker's
`allowance`, `late` and `silent` since the last reset (counted from the reset call); the remote
client reads them; `oscairn-engine-timing` prints a line per worker. Nothing that plays changed.
Tests: `control-session` (the outcome rule with given times), `worker-failures` (a 2 ms plugin
against the default allowance is silenced; late and silent are deadline misses; the reported
allowance), `control-local-stream` (both counts cross the socket), `control-json-*`
(`json_rpc.py`'s status shape).

### Step 3 status

Built 2026-11-13, experimental and off by default. `OSCAIRN_WORKER_EXTRA_BLOCK` lists plugin
IDs; each instance of one in a worker, in real time, is in the state for its life (read as it
loads). All of it is in `WorkerHost` (`startDelayed`, `finishDelayed`): two buffers of the
worker's block, one for the block handed and one for the block playing, so a block that cannot be
handed never overwrites the one playing; the wait for the last block happens as the next starts,
for what is left of the allowance plus a period; its output events, voice report, reset and
result come with it, so a panic or a note off takes effect exactly one block late, as everything
else does. The first block after loading is silent and processed; after a worker restart nothing
the old worker rendered is played. Offline rendering is never delayed. `roundTrip` in
`engine.timing` then spans about a period by construction; `block` in `session.status` (the time
inside the worker) is the one to read.

Test: `worker-extra-block`: against the same plugin in the engine, 40 blocks with notes on and
off and a panic come out one block late sample for sample with their events and results, through
`process()` and through `startProcess`/`finishProcess`; an effect in another worker beside it is
not delayed; a worker crashing on a note restarts, nothing stale plays, and a new note sounds a
block later as from a fresh instance; a graph of the Basic Synth into an effect, each in a
worker, comes out one block after the same graph without it, in waves and one step at a time.
A Performance change needs nothing more: a new instance starts with nothing to play, and one
retiring plays its last blocks a block late like the rest (reasoned, not tested).

### Transparent huge pages stall a worker's audio thread

Measured on the FANTOM-07 on 2026-11-13 (Linux 4.19, memory locked, audio threads SCHED_FIFO,
transparent huge pages on `madvise`): every 10.24 s, the grid of khugepaged's scan (`scan_sleep`
10 s), a worker's audio thread stalls for 5 to 23 ms while not running (its CPU time is a
fraction of the block's wall time, with one page fault per context switch). That fits
khugepaged holding the process's mmap lock while it collapses a 2 MB range, the audio thread
blocking in a page fault; the workers' 2 MB heap regions and 16 MB of the engine's carry the
`hg` flag. The fix, `disableTransparentHugePages` (`prctl(PR_SET_THP_DISABLE)`, from the engine,
which its workers inherit, and again from each worker's start), is built; `OSCAIRN_THP=keep`
leaves a process as it is, for measuring. Confirmation trial (transparent huge pages off
system-wide, then this change) pending.

### Step 4 status

Built 2026-11-13, the owner's first decision. What a missed block does now:

- **A block handed but not done within the allowance** is silenced and the worker goes on
  rendering it, as before, but the plugin is not panicked and the part's note routes are kept:
  the host reports it as not processed with its events kept (`PluginHost::eventsKept`, new), and
  the graph (and the prepared instrument's instances) reset only where events were lost. The late
  block's sound is discarded; its output events (note ends, a note effect's notes) are delivered
  at the start of the next block handed, so a note effect's releases are not lost either, and a
  reset the plugin made in it counts as the next block's.
- **A block not handed because the worker is still busy** has its events carried to the next
  block handed, at time 0 and in order, before that block's own; SysEx bytes copied into a pool of
  their own. The carry holds one block's events (`maxEvents`, a graph block's capacity). Decided
  while building, as the owner asked: when it is full, the oldest event that is not a release goes
  (a note on, a parameter value, a controller), counted in `droppedEvents`; a release (note off or
  choke, a MIDI note off, sustain pedal up, all notes or sound off) is never dropped for another
  event. Only when the carry holds nothing but releases is a release lost, and then the plugin is
  reset, as an overflow is today: the block reports its events lost (the graph panics), or, when
  the loss happens merging the carry into the block being handed, the worker resets the plugin
  before that block, which reports it as `wasReset`.
- **The fade:** a silenced block starts from each channel's last sample heard and fades it to zero
  over about 1 ms (48 frames at 48 kHz, continued across blocks shorter than that), then silence;
  the next block heard fades in over as long. A restart's 10 ms fade-in is unchanged. No
  allocation on the audio thread: the carry, its SysEx pool and the last samples are allocated
  with the node.
- **The extra block** follows the same rules: the block played's result carries whether its
  events were kept; a panic still waits for the next block handed (commit 9ee713bf's ordering).
- **A control silence stays a reset.** The worker's own host drops a block's events when its main
  thread holds the control gate, so that block lost its events and the panic after it stays, as
  the policy says ("panic only where events were really lost"). Found only when a late block's
  result comes in (it was a control silence there), the reset goes to the plugin before the next
  block handed.
- **"Too slow" is unchanged** (step 5 is a separate decision): it still counts blocks silenced
  after waiting and played late, a worker that hangs is still failed at 250 ms, and carrying events
  does not touch either count. `worker-failures` still restarts the 2 ms fixture as too slow.

Tests: `worker-miss` (new): the fixture stalling 100 ms in one block against a 10 ms allowance,
in a graph: a note held through it sounds afterwards at its level, with no panic (`missPanics` 0);
the dropout's largest step between samples is the sound's 48th (a cut's is the whole sound); a
note off sent while the worker is busy takes effect afterwards (the tail instrument's release
decays to silence); a note on sent then sounds afterwards; two parts sharing one Basic Synth
(stalled by a test build of the worker, `oscairn-stalling-worker`) both keep their notes; the
three single-part cases again with the extra block; the carry overflowing with controllers keeps
the note off and drops the oldest of the rest, and overflowing with releases reports the block's
events lost. Each check fails with the old panic, without the carry or with a cut instead of the fade, whichever it is about (checked by putting each back). `control-worker-timing`
now expects no panic after the stall's blocks.

## Core placement

What the device showed: interrupts on core 0, nothing pinned, and the display's software
rendering stalling the engine and two workers together for 3–6 ms. With nothing pinned, the
scheduler is free to put the compositor, Xwayland and the app's drawing thread on the same core as
an audio thread; the audio threads (FIFO 70–80) preempt them, but a non-real-time kernel holds
locks and runs softirq and memory work (page reclaim, the compositor's buffer traffic) that FIFO
priority does not preempt. Whether the 3–6 ms came from shared cores, memory bandwidth or a
kernel lock is **unverified**; pinning (below) is the cheap experiment that separates them.

Proposed (step 7):

- **Audio threads off the interrupt and display core.** On the FANTOM-07: audio on cores 1–3
  (`compute.audioThreadCores` exists, built 2026-11-12, off by default), and the display stack
  (Weston, Xwayland, the app's UI) on core 0, set by the session (`taskset`/a cpuset for the
  shell), not by Oscairn's threads. The RFC 0035 cpuset partition (`os/scripts/audio-cores.py`,
  the session's `oscairn-audio` partition, `irqaffinity=`, `nohz_full=`) is the image's version
  of the same, verified only in QEMU.
- **Not a core per thread.** The engine's thread mostly sleeps while workers render; workers of
  one wave want different cores, but which ones are busy changes block by block. Pinning each
  worker to one core would put two heavy workers on one core with others idle. Let the kernel's
  real-time scheduler place them within the audio cores (it pushes a runnable FIFO task to an idle
  core). Pin only when one instance is known to need a core alone (a split instance, below), and
  then by the placement step, not the profile.
- **Two cores:** core 0 for the system and display, core 1 for all audio. Parallel waves gain
  nothing there and each worker costs a wake-up on the same core, so the bias moves to sharing
  and to plugins in the engine where trusted (RFC 0010's placement policy). The allowance from the
  deadline (step 6) matters more than ever.
- **Eight cores:** two for the system and display, six for audio; the helpers' cap of three
  (`AudioHelpers.cpp`) should become the audio cores less one.
- **One profile field.** Today `realtimeCores` is a count (the last N cores, used by
  `audio-cores.py` for the partition, `nohz_full` and `irqaffinity`) and `audioThreadCores` a list
  (thread affinity). They describe one thing. Proposed: `realtimeCores` becomes the list
  (RFC 0035 already planned that), `audioThreadCores` goes, and the threads keep to the list
  wherever the image's partition is not there to do it. Pre-release formats break freely, so no
  compatibility reading.

## Share by default, split under load

### Where it is decided today

- **The host decides, with no setting anywhere** (not per profile, Performance or part).
  `Session::add` asks `sharingSource` (`core/control/src/session.cpp`): a new part shares an
  instance with the same plugin ID and the same file path that has room; `shareInstrument` adds
  an instance when none has. Room (`SoundContext::room`, `core/engine/src/sound_context.cpp`) is
  the smallest of free output buses, free MIDI channels and free sound contexts. A Performance's
  load forms sharing again (`joinEngine` and `sameKind` in `core/control/src/performance.cpp`),
  and the next Performance's sounds are prepared in the playing instances when they have room.
  A plugin can share when it offers native sound contexts, a sound saved on its own, several
  output ports and a stepped output-bus parameter: FM (8 contexts), Analog (16) and the Sampler
  all share in today's builds, as does the Basic Synth (8).
- **Nothing can give a part an instance of its own** while another of the same plugin has room
  (RFC 0021 says so). The only way today is loading the plugin from byte-identical copies at
  different paths, a test trick that also defeats instance take-over between Performances.
- **A shared instance is one worker, one audio thread, one core and one allowance:** its parts
  render one after another in one block request. A silent block silences every part on it, and
  the panic of [what a miss does today](#what-a-miss-does-today) ends every part's held notes.
- Placement is per plugin ID only: in the engine or in a worker (`PlacementPolicy`). Nothing
  looks at CPU or cores.
- RFC 0025's CPU budget is one pooled number, a share of one audio thread's time, with no per-core
  notion.

### Fair voices between the parts of one instance

A requirement of its own, apart from load: **sharing must not starve a part.** The voice pool is
the plugin's and all its contexts draw on it (FM: one pool of 16; Analog: one store of 2 × 64
Surge voices; the Basic Synth: 128). The host's voice budget is one figure, measured once per
instance, with each part's reserve and priority passed through `oscairn.voice-budget`; but with no
budget set it does nothing, and the per-context `limit` it passes is 0 ("the host keeps each
part's note limit itself", `voice-budget.h`). Observed in a container (not CPU-dependent; a code
reading and test kit of 2026-11-13, `reports/Oscairn engine sharing and voices.md` outside the
repository): two FM parts sharing one instance with 16 keys held gave the first part 0 voices and
the second 16; with four parts the first three were silent or nearly; four separate instances
held 16 each. FM and Analog give up voices themselves, so the host's `stolen` and `chokes` stay 0
and the Mixer shows nothing. Sharing today can silence whole parts, not just lower the total.

Proposed (step 9a, before any load-based split):

- **A fair share per context, always:** the host passes each context a limit even with no CPU
  budget: its part's `maxNotes` if set, else the plugin's pool divided among the parts sounding,
  with each part's reserve (RFC 0018) guaranteed first. The extension already carries a limit per
  context; the plugins must honor it (FM, Analog and the Basic Synth implement the extension; that
  they honor a per-context limit, not only the instance's, is to be checked per plugin).
- **A host rule for plugins that do not:** where a plugin ignores per-context limits, the host
  counts notes per part (it already does for `maxNotes`, `NoteRoute::route`) and steals within the
  part that is over its share before the plugin's own stealing reaches another part.
- **Reported:** `parts.meters` gives each shared part its own voices and steals, which it does not
  today for shared nodes.

Tested with the test kit's two- and four-part shared instances: every part keeps at least its
share at 16 keys, with and without a CPU budget.

### Splitting under load

- **The signal:** each instance's block time inside its worker, its p99 over the last second
  against its allowance (step 2's counts and the round trips; step 11 adds it to the report).
- **The action:** start a second instance of the same plugin in a new worker, prepare the part's
  Sound in it (RFC 0021's prepare beside, off the audio thread), then move the part: new notes go
  to the new instance while the old one plays out the notes it holds (the existing ring-out of a
  retiring sound). No gap: nothing is cut, and the part's parameter values, channel, bus, zones,
  note limit and meter live on its node, not in the instance (`sound_context.h`). What does not
  move: the old instance's voices (they ring out), and any state the plugin keeps per instance
  rather than per sound context (which the plugin's snapshot of the Sound carries).
- **Which part moves:** the part with the most voice time (the voice report's per-note cost,
  RFC 0018) that is not the only part on its instance.
- **Merging back** under low load is the same move in reverse, done only when the moving part is
  silent.
- **The profile's bias:** `compute.sharing`: `memory` (share until the instance is past its
  allowance, the default on 1 GB devices), `balanced` (split at 60% of the allowance), `cpu` (an
  instance per part when memory allows, the desktop's default). Proposed values; to be measured.
- **Memory:** a split is an instance's shared bytes again (`oscairn.memory`'s `shared_bytes`, the
  samples or wavetables) plus the part's context bytes. It is refused, and reported, when that
  does not fit the committed share (RFC 0025's memory classes); the part then stays shared and the
  instance sheds voices.
- **Voice limits follow the parts:** each part's share (above) is its own; the instance's limit
  becomes the sum of its parts' shares, capped by the host's voice budget, which stays the real
  limiter. A split moves the part's share with it, and the part gets a whole pool.
- **The hooks:** `Session::sharingSource` (adding a part), `Session::read` (forming sharing on
  load) and `SoundContext::room` (the room the bias narrows); a part pinned to an instance of its
  own is a per-part flag in the Performance read there.
- **Memory, indicative:** about 144 MiB for four Sampler parts sharing one instance against about
  339 MiB for four instances (measured in a container, not on the device).

### Can two Sampler instances share loaded samples?

Not today, as far as the repository shows (the Sampler's source is in its own repository and was
not read for this RFC: **unverified** beyond these documents): each instance is its own worker
process, and sfizz preloads the start of every sample into its own heap and streams the rest on
its own threads; RFC 0025 counts the sample files as the kernel's page cache, which processes do
share, but the decoded, preloaded floats are private to each. Sharing them would need the plugin to
map read-only sample data that the kernel can share (files already in the playback format, mapped
with `mmap`), or a host service handing out shared, read-only sample memory (an `oscairn.assets`
extension). Either is a plugin change; this RFC records the question for the Sampler's own work
and does not design it.

## Start-up and first-use stalls

What the host can require or do (step 10):

- **A Sound is playable only when prepared:** `oscairn.sound-contexts`' prepare runs on the main
  thread and "may read files" (`sound-contexts.h`), so loading is already off the audio thread for
  plugins that use it. What is missing is a way for a plugin to say it is still loading in the
  background (sfizz's streaming threads, a wavetable cache). Proposed: an `oscairn.ready` draft:
  `is_ready(context)` and a callback when it becomes ready; until then the part is shown as
  loading, its notes are not sent (or are sent and the part muted), and a Performance change waits
  for it like any prepare. A plugin without the extension is ready when prepare returns.
- **Warm-up blocks:** before a new instance goes live, the worker renders a few seconds' worth of
  silent blocks and, if the plugin allows, a silent chord at zero gain, on its audio thread, so
  first-touch page faults, lazy tables and allocations happen there. The engine already keeps the
  first 512 blocks out of the steady timing (`BlockTiming`); this moves them before the first note.
- **Memory locked and touched:** at `instrument` level the engine and workers lock their memory
  (RFC 0025); locking does not fault in pages a plugin allocates later, so warm-up matters even
  then.
- **What belongs to plugins:** allocating, loading files or building tables on the audio thread at
  the first note is the plugin's to fix; the porting guide should say so with the measurement.
  Analog's 7–16 ms stalls at one note are far past any allowance or extra block; their cause in
  the Surge code is **unverified** (allocation, wavetable loading and denormals are the usual
  ones) and needs a profile on the device. The Sampler's first-chord stall is consistent with
  sfizz waiting on its streaming threads for samples whose start was not preloaded; also
  **unverified**.

## What the timing tools must report

Steps 2 and 11; built marked as such.

- Each worker's allowance (built) and, with step 6, the allowance it got per block (a histogram).
- Blocks past the allowance by cause: played late (built), silenced after waiting (built, as
  silent), silenced because the worker was busy (built: `busy`, within silent), the
  plugin's own block longer than the period (in `deadlineMisses`).
- Panics and note cuts caused by misses (built, `missPanics`, the panics sent right after a block
  the worker did not render; since step 4 none follows a late or busy block, only one whose events
  were lost: a control silence in the worker, or a carry that had to drop a release).
- Each part's latency state: normal or one block late, since when, and how often it entered and
  left (step 8).
- Each instance's parts and its share of their voice time, for sharing (step 9).
- The wake-up cost of each worker: its round trip less its block time inside the worker, which
  shows a worker waiting for a core (step 7's measurement). Built: the round trip taken apart,
  `collect` (until the engine's thread came for the block), `wake` (until the worker's audio
  thread ran) and `run` (the worker's whole block, the panic's reset included), each a histogram
  in `engine.timing`; with each worker's audio-thread page faults (`minorFaults`, `majorFaults`).
  The audio thread's own CPU time per block (`cpu`, a histogram beside `run`, whose largest value
  carries the cpu of the same block as `maxCpu`) and its involuntary context switches
  (`involuntarySwitches`) tell a run that was computing from one that was not scheduled.
  Tested by `control-worker-timing`: one block stalled in the plugin, every part apart, the cpu far
  below the run for a stall that sleeps and close to it for one that spins.
- The callback's duration and lateness, and the device's underruns, as now.
- The same cpu histogram keeps a stall out of the admission estimates ([RFC 0025](0025-resource-budgets.md#status)):
  a plugin's cost is a running mean of its blocks' wall time held to the CPU time its audio thread
  used, so a block the kernel did not schedule counts for the work done and no more, and notes
  held do not count as cost at rest. The first estimator took the largest block seen, so on the
  FANTOM-07 the stalls and held notes of one night's test runs made Analog cost 211% of the audio
  time at rest and refused nearly every Performance; `budget.forget` lets go of such figures.
  Tested by `control-estimates`.

## Decisions for the owner

Each with a recommendation.

1. **The miss policy (step 4): stop ending a part's notes on a missed block.** Recommended: yes,
   first, before any other change; it is a behavior fix more than a policy.
2. **"Too slow" (step 5): restart only a worker that has stopped making progress;** shed voices,
   then use the extra block, for one that is merely slow. Recommended: yes.
3. **The allowance from the block's deadline and the graph's waves (step 6),** behind a setting
   until measured, then the default. Recommended: yes.
4. **Adaptive latency: the per-instance extra block, entered and left under measured load
   (step 8),** with a profile list of plugins that start in it as the stopgap. Not switching the
   device buffer. Recommended: yes, after the device has tried step 3 on Analog and the Sampler.
5. **Part taps in a recording shifted back for a delayed part; the main mix recorded as heard.**
   Recommended: yes.
6. **One profile field for the audio cores:** `realtimeCores` as a list, `audioThreadCores`
   removed; the FANTOM-07's profile sets cores 1–3 and its session keeps the display stack on
   core 0. Recommended: yes, after one measurement with `audioThreadCores: [1, 2, 3]` and the
   display drawing.
7. **The sharing bias names:** `compute.sharing` as `memory`, `balanced`, `cpu`, with `memory`
   on 1 GB devices. Recommended: yes, the thresholds to be measured.
8. **`oscairn.ready` and warm-up blocks** as the start-up answer. Recommended: yes; the draft
   header goes to the plugin SDK with a fixture before any real plugin adopts it.
9. **Fair voices between shared parts (step 9a), as a correctness fix before any split:** each
   part a share of the plugin's pool even with no CPU budget, its reserve first. Recommended:
   yes, first among the sharing work.
10. **Sample sharing between Sampler instances:** left to the Sampler's own work (a plugin
   change), not designed here. Recommended: yes.

## How each step is tested

- Steps 1–3: as their status sections say (built).
- Step 4: a graph test with a worker that misses one block chosen by the test (a fixture mode
  that sleeps past the allowance in a block it is told to through a parameter), checking that its held notes keep sounding
  after the miss, no panic reaches it, the events of a block not handed arrive in the next, and
  the fade's samples; `worker-failures` still names a crash, a hang and memory.
- Step 5: a worker that renders every block in 1.5 allowances is never restarted and has its
  voice limit lowered; one that stops is restarted after a quarter second.
- Step 6: with an injected callback start and step costs, each worker's wait in a chain and in
  parallel parts; on the device, `engine.timing` before and after with the Sampler at 32 and 48
  notes: fewer silent blocks and no new `audioUnderruns`.
- Step 7: in the Linux container, the threads' allowed CPUs read from `/proc` (as for
  `audioThreadCores`); on the device, the drawing measurement repeated with the display on core 0.
- Step 8: with an injected clock and block times, entering at the threshold, staying through the
  hysteresis, leaving only when silent, and the sample-exact shift of step 3's test across an
  entry and an exit.
- Step 9a: as [fair voices](#fair-voices-between-the-parts-of-one-instance) says.
- Step 9: a session test with a fixture whose block time the test sets: the split, the part's new
  notes on the new instance while the old rings out, no gap in the mix, the refusal when memory
  does not fit, voice limits moving with the part.
- Step 10: a fixture that reports not ready for a while: its part shows loading and gets no
  notes, then plays; warm-up blocks before the first note in the worker's timing.
- Step 11: the report's fields in `control-session` and over the socket, as step 2.

## Ruled out

- **Switching the device's block size automatically:** a device reopen, a gap, and every part
  pays ([above](#against-switching-the-device-buffer-and-against-a-fixed-setting)).
- **Repeating the last block, with decay, on a miss:** a buzz at the block rate on pitched sounds.
- **A core per worker, pinned by the profile:** wastes cores as load moves between parts.
- **A smaller fixed share per plugin for long chains** (RFC 0010 step 3's note): the deadline
  rule replaces it.
- **Restarting a slow but working plugin:** loses its voices and does not make it faster.
