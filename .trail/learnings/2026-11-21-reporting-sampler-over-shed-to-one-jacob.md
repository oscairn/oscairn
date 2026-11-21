---
title: "Reporting Sampler over-shed to one voice: held keys whose voices ended counted as sounding"
author: Jacob
date: 2026-11-21
branch: voice-overshed
cost: { minutes: 2006, tokens: 7874363 }
id: l-2ca04e82
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/route.rs
  - core/engine/rust/src/budget.rs
  - core/engine/rust/src/notes.rs
  - core/engine/tests/core_importance.cpp
  - core/engine/tests/voice_scenario.cpp
---

## Decided
- The unit (2026-11-20, Sampler 0.9.7, pool one host): 30 keys struck under the pedal and NEVER let go (the take sends note-ons only), lane 870 of 1066 us; the piano gave up 61-65 and sat at one voice with the lane at a third. Cause: a held key keeps its slot after NOTE_END (route.rs ended sets voice_ended) and the budget counted those slots (used & !stolen) in estimate, victims, node_notes, per_note_scale's live and the admitted voices. Each voice given up, ended by key assign Single or cut from a full 64-voice pool stayed in the estimate at the per-voice cost measured over the voices really sounding, so each new note took one more and the estimate never fell. NoteRoute::voiced now leaves them out (non-reporting plugins too: engine-core.vec recorded at b6fa3f2b). Chain heavy's 318 is the same cause (the pool full at 64 voices, ~80 host notes).
- Admission takes a listed held/pedaled voice only past the lane's limit (the owner's rule), released ones still from SOFT.
- importanceMismatch: over every cap given since (max of checked and cap) and only while the context still lists a voice; over_limit for a listing plugin reads the report's kept voices, not active_voices (which counts fading units).
## Ruled out
- settle, the 16-entry unlisted stop, the admit counter, judge_held, weight-to-time and voice_cost (only a seed before the first measurement): none drives the collapse; the model reproduces it with all of them as they were.
## Watch out
- Why the Mac tests missed it: every test let its keys go (note-off under the pedal), so NOTE_END freed the slot; only keys held without note-off keep voice_ended slots. Model takes should include held keys.
- Graph::process without worker::callbackDeadline gives the core no slack: a lowered cap then comes back only after 10 s healthy. Worker tests of recovery must set the deadline (voice_scenario's unit cases do).
- A pool process keeps the environment it started with: fixture settings that differ per case need a worker per plugin or a separate process.
- On a late lane (OSCAIRN_WORKER_EXTRA_BLOCK) hear() resets Heard::admitted on a report one render old, so a cap set then misses the last block's new note (one voice over-shed per take); not fixed.
