---
title: "Voice importance: step 3's lead lost its own key, and the report never reached the core"
author: Jacob
date: 2026-11-20
branch: voice-importance
cost: { minutes: 1962, tokens: 5457513 }
id: l-419cdd00
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/budget.rs
  - core/engine/rust/src/notes.rs
  - core/engine/rust/src/lib.rs
  - core/engine/tests/core_importance.cpp
  - core/engine/tests/voice_scenario.cpp
---

## Decided
- RFC 0046 step 3's fault (pool, piano and lead in one worker: 1 piano and 1 lead voice taken, not ~16 piano): a listing plugin's context cap was its reported voices less those taken (budget.rs take_listed via listed_left), so a note admitted after the report arrived over the cap and the plugin's own victim() gave up its held key. Now Heard::admitted counts voices admitted since the report (admit adds 1 before it takes anything), admit makes room for a listing plugin's new note at voice_cost (one unit, not note_now: per_note_scale = voices/live can be 4 when released voices outlive their host notes), and a listing node's instance limit is floored at what it sounds.
- Lane::settle (3 blocks) after listed voices go: the plugin drops them next block and its report at once, but the lane's took is the median of five blocks; judging again on it took 25 for an excess of 15.
- choose_on's `unlisted` bound: a context whose 16 listed voices are spent while more sound unlisted stops the merge above its last listed key (else the lead's held key was next).
- Plugins::importance (lib.rs) read `lists` as an argument before the call that sets it: every real graph got an empty report since step 1. Only a real-worker test shows it (core_importance drives notes_finish directly; voice-importance asks the host).
- Step 4: held needs the lane's own CPU within its limit AND evidence of being held off (run - cpu past the soft margin, or the least wake wait of its plugins past 25% of its time). Max wake wait was wrong: in a pool the second plugin's wait includes the first's render; a lane that overran waits at its next wake behind itself.

## Tried and ruled out
- The voice fixture's OSCAIRN_FIXTURE_SUSTAIN never got the pedal: the Rust CLAP host's accepts() sends CLAP_EVENT_MIDI only to a port with the MIDI dialect; the fixture declared CLAP only, so its "pedaled" notes rang out 1 s after note-off (guard-pedalled passed regardless). Fixed by declaring MIDI too.
- Judging held without CPU evidence (cpu <= limit alone): a lane whose median lags an overload that just ended looked held.

## Watch out
- judge_held reads CPU below run as preemption; a worker that sleeps inside its run (waiting on a fused source in another process) would look held too. Check before giving such a lane voices to shed.
- Pool hosts take their environment when they start and stay resident across Graphs: per-instance fixture settings in a pool need a per-channel switch (OSCAIRN_FIXTURE_CHANNEL_US), not setenv before add.
- core_importance models limits after routing (run.rs: prepare, then limit_voices); setting them before routing reproduces a fault the host does not have.
