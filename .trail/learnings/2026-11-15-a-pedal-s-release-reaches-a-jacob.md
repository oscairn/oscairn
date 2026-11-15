---
title: "A pedal's release reaches a part from any source, compared with what was last sent on, so a receive edited while the pedal is down cannot strand it"
author: Jacob
date: 2026-11-15
branch: guard-limits
cost: { minutes: 234, tokens: 266403 }
id: l-67814017
scope: repo
status: active
src: agent
touches:
  - core/CMakeLists.txt
  - core/engine/rust/src/budget.rs
  - core/engine/rust/src/notes.rs
  - core/engine/src/core_cpp.cpp
  - core/engine/tests/guard.cpp
  - core/engine/rust/src/lib.rs
  - core/engine/src/NoteRoute.h
  - core/engine/src/engine_core.h
  - core/engine/src/graph.cpp
  - core/engine/tests/core_diff.cpp
---

## Decided
A part's pedal delivery compares the value it would send (the largest of the sources its receive takes) with the value it last sent on that channel, not with the largest before this event. A pedal-up from any source is recorded even when the receive no longer takes that source; pedal-downs from untaken sources or other channels are still dropped. Routes without zones record what passes too, so zones added later start from the truth.

## Tried and ruled out
Comparing with the largest before the event under the current zones: after the receive changed while the pedal was down, the old pedal-down is no longer counted, so the pedal-up changed nothing and was dropped; the plugin kept the pedal down until the receive was put back.

## Watch out
An edit made while a pedal is down still lets go only when that pedal comes up (no event is sent at the edit itself). A live edit made before the pedal goes down took effect already (the session commits the zones; parts.input commits as an Edit); the device report of no effect was not reproduced off the device.

In the code: core/engine/rust/src/route.rs pedals_sent, core/engine/src/NoteRoute.h pedalsSent; test core/control/tests/parts.cpp pending-pedals "live".
