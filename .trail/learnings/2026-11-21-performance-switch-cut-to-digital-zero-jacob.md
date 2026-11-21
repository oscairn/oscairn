---
title: "Performance-switch cut to digital zero: the recovery record's whole-instance save reset the shared engine"
author: Jacob
date: 2026-11-21
branch: ringout-3
cost: { minutes: 13, tokens: 118198 }
id: l-5ba8069f
scope: repo
status: active
src: agent
touches:
  - core/plugin-host/src/WorkerHost.cpp
  - core/control/src/parts.cpp
  - core/control/src/performance.cpp
  - core/engine/src/graph.cpp
  - core/control/tests/performance_switch_stress.cpp
---

## Decided
- A held, pedaled tail cut to digital zero at a Performance change, intermittently and with no notice, came from the plugin host, not from a Session ring-out path. The incoming parts join the playing shared engine (Session::read's joinEngine: prepareSnapshot beside the old sounds), and WorkerHost::prepareSnapshot then saved the whole instance as the worker's recovery record (record(Op::SaveSnapshot)). That save holds the CLAP host's gate (a sound-contexts snapshot excludes rendering), so a block arriving meanwhile is a control silence in the worker (s.ok false, kept_ false); the Rust core treats it as lost (`lost = !processed && !kept`, run.rs) and panics the plugin: a CLAP reset that ends every voice of every sound on the engine, the ringing tail included. Whether a block fell inside the save was timing: intermittent; longer saves on the A53 made it likelier.
- The record of a snapshot prepared beside is now taken once the plugin sleeps (WorkerHost::recordAsleep, from prepareSnapshot and service); while it saves, start() hands no block (saving_), carrying the events, so a note struck then plays after the save instead of being lost to a reset. Until then restore() loads the last record and prepares the pending snapshots again (pendingBeside_), each one's discarded sounds going at once; one that no longer fits is left out with a log line instead of failing the restart.
- engine.timing's workers now give controlSilences, and session.status ringOut counts each way a tail ends at a change (cut, faded, chokedOlder, partial, panicEnded, limitEnded, fadeEnded), so the next unit run names the path.
- One route per set of live gains (Graph::connectAudio matches live slots; compileWires keys routes by channels and slots): master and faders stay live with a part's send to an empty bus at any level.
## Tried and ruled out
- Reproducing with the Basic Synth at device pacing (2.67 ms blocks), in process or in chain workers: 0 cuts in 80. The real Sampler (built from the pcm repo, OSCAIRN_RINGOUT_SAMPLER and a samplepack-made sine.osp) in chain workers cut 1 in 12 to 40; the Basic Synth cuts too once blocks come every 300 us (a save then overlaps one): 5 of 60. In process it never cut: only WorkerHost keeps a recovery record.
- Sample sharing between the two Performances: cuts stayed with separate sample packs.
- The voice budget and the load guard: the cut was a CLAP reset (a debug print in the Sampler's silence()), right after "node 0 not rendered (control op 16 = SaveSnapshot)".
## Watch out
- Any other whole-state save or load on an instance while it sounds (saving a Performance while notes ring: writeDocument's saveSnapshot on a shared engine; a preset load) still resets it the same way: a control silence is followed by a reset whether or not the lost block carried a release. Not changed here.
- A restart's replay cannot discard a record's sound that the record selects (pins); two changes with notes ringing before the engine sleeps can leave a pending snapshot that no longer fits.
- control-performance-switch-stress's race kills workers once (not in plugin hosts, whose restarts within a minute the later kill sections count).
- control-performance-switch-stress-hosts hung (CI timeout at 120 s) on descriptor inheritance, not on the fade: a worker spawned by another thread inherited another worker's socket (killed, it then closed nothing; a request to it waited the 30 s reply timeout) or the test's popen pipe (its pgrep read never ended). Workers now inherit only their socket and 0-2, and descriptors are made close-on-exec under one mutex with the spawns. Found with a 20 s watchdog and `sample`; a hung run's main thread sits in WorkerHost::request's recv while every worker idles in poll.
- Session::service must never throw: keepWarm asked a just-killed worker for effect resources before checking its failure.
