---
title: "A plugin's limit lowered for lost blocks is held there while notes are held, not taken off a limit that regrows"
author: Jacob
date: 2026-11-15
branch: guard-limits
cost: { minutes: 29, tokens: 32787 }
id: l-b64039ef
scope: repo
status: active
src: agent
touches:
  - core/control/CMakeLists.txt
  - core/control/tests/parts.cpp
  - core/engine/rust/src/route.rs
  - core/engine/src/NoteRoute.h
  - core/CMakeLists.txt
  - core/engine/rust/src/budget.rs
  - core/engine/rust/src/notes.rs
  - core/engine/src/core_cpp.cpp
  - core/engine/tests/guard.cpp
  - core/engine/rust/src/lib.rs
---

## Decided
When lost blocks or the deadline take a voice off a voice-budget plugin's limit, the guard sets a cap one below the limit then in force, and the plugin's limit is the smaller of its usual rule and that cap until its notes are all let go. Before, the guard counted voices cut and subtracted them from the usual rule (admitted voices plus room); the room grows back as soon as the shed voices stop costing, so the limit climbed back and the losses came back step by step (the device's Sick tines report).

## Tried and ruled out
A guard case with notes arriving all the while (a new note every 2 blocks per part) did not tell the two rules apart and was flaky near its threshold; dropped. The held starved case at effect level 0.4 does: the old rule lost 10-11 blocks from block 500 to 800, the cap 2.

## Watch out
The cap stays while any note is held, so a part played without pause keeps the lowered limit until the player lets go; giving voices back while notes are held only brought the losses back.

In the code: core/engine/rust/src/budget.rs Entry.cap, give_up, calm, voice_limit; core/engine/tests/guard.cpp starved.
