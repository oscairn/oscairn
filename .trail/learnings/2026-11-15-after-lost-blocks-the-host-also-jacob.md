---
title: "After lost blocks the host also caps a plugin without the voice budget at the notes it held, so re-struck keys do not bring the losses back"
author: Jacob
date: 2026-11-15
branch: guard-chain
cost: { minutes: 30, tokens: 15352 }
id: l-36c7aa23
scope: repo
status: active
src: agent
touches:
  - core/control/include/oscairn/control.h
  - core/control/src/json_rpc.cpp
  - core/control/src/remote_control.cpp
  - core/control/src/session.cpp
  - core/engine/include/oscairn/graph.h
  - core/engine/rust/src/budget.rs
  - core/engine/rust/src/lib.rs
  - core/engine/src/core_cpp.cpp
  - core/engine/src/engine_core.h
  - core/engine/src/graph.cpp
---

## Decided
Trouble (a voice given up for lost blocks or the deadline) on a plugin without oscairn.voice-budget now sets the 12k ceiling at the notes the host holds for its node (every connection, every sound of a shared instance) and caps one under; a note into a capped node first fades the node's quietest released note. The cap returns by 12k's rule and clears when the notes are let go.

## Tried and ruled out
12l alone on the unit (Sick tines pedalled, FM without the extension): each sparse loss faded a note and the next re-strike brought one back, so about 2 losses a second continued with voices hovering. The ceiling/cap existed only for own-voices plugins.

In the code: core/engine/rust/src/budget.rs Budget::capped, node_notes, choose_on, Entry::trouble(limit); core/engine/tests/core_recovery.cpp chain.
