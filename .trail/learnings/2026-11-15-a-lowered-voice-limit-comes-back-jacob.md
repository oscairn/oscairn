---
title: "A lowered voice limit comes back while notes are held: fast up to just under where it hurt, then probing slowly, the ceiling forgotten after a healthy stretch"
author: Jacob
date: 2026-11-15
branch: guard-limits
cost: { minutes: 21, tokens: 29074 }
id: l-fe2eb48b
scope: repo
status: active
replaces: l-b64039ef
src: agent
touches:
  - docs/plugin-porting.md
  - docs/rfcs/0045-audio-scheduling-on-small-devices.md
  - core/engine/rust/src/budget.rs
  - core/engine/rust/src/notes.rs
  - core/engine/tests/guard.cpp
  - core/control/CMakeLists.txt
  - core/control/tests/parts.cpp
  - core/engine/rust/src/route.rs
  - core/engine/src/NoteRoute.h
  - core/CMakeLists.txt
---

## Decided
After lost blocks or the deadline lower a voice-budget plugin's limit, the guard remembers a ceiling (the limit in force at the trouble) and caps the limit one under it. On healthy blocks (no loss on the lane in its last eight, the lane within 85 % of its limit, slack fine) a voice comes back each 8 blocks below ceiling-1 and each 750 blocks (2 s) above it, the probe; a failed probe (trouble after a probe's wait, at or past the ceiling) resets the ceiling and doubles the wait, at most twice. The ceiling is forgotten after 3,750 healthy blocks (10 s) or once the cost per voice moves 25 %. All of it clears when the plugin holds no note. Rust core only, like 12b-12d.

## Tried and ruled out
Holding the lowered limit until every note was let go (commit c884028f): the owner rejected it, since a pedal held through a long passage keeps a part at reduced polyphony for minutes. Taking voices off the usual rule (admitted + room - cut): the room grows back as soon as the shed voices stop costing, so the limit climbed straight back and the losses returned (Sick tines). Making measured lane overload set the ceiling: it would change voice limits the C++ reference computes, breaking the differential test, so it is left out.

## Watch out
The drop itself takes several troubles a block apart with the limit a block behind; counting those as failed probes doubled the probe wait at once, so a failed probe needs a probe's wait of health before it. A guard case with notes arriving under real worker timing was flaky; the deterministic test drives the core directly with the plugin's cost a function of its voices.

In the code: core/engine/rust/src/budget.rs Entry::trouble, Budget::recover, RECOVER_BLOCKS, PROBE_BLOCKS, FORGET_BLOCKS; core/engine/tests/core_recovery.cpp.
