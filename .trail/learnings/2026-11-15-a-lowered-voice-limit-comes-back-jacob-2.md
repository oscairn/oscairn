---
title: "A lowered voice limit comes back only when the measurements say the next voice fits, never by trying and losing a block"
author: Jacob
date: 2026-11-15
branch: guard-probe
cost: { minutes: 26, tokens: 23853 }
id: l-27917037
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/budget.rs
  - core/engine/tests/core_recovery.cpp
  - docs/plugin-porting.md
  - docs/rfcs/0045-audio-scheduling-on-small-devices.md
  - core/control/include/oscairn/control.h
  - core/control/src/json_rpc.cpp
  - core/control/src/remote_control.cpp
  - core/control/src/session.cpp
  - core/engine/include/oscairn/graph.h
  - core/engine/rust/src/lib.rs
---

## Decided
12k's raises (below the ceiling and probes past it) now need the lane's recent worst block plus the node's cost per note within 85 % of the lane limit, and the callback's recent worst slack less that cost at least 15 % of the deadline ("recent worst": the largest, falling 1 % a block). Without slack, cost or a lane limit, a voice comes back only after 10 s healthy. A probe that still loses a block doubles its wait until release. The ceiling is per node: a shared instance's sounds share it.

## Tried and ruled out
Probing by time (a voice every 2-8 s above the ceiling): each probe cost a pair of lost blocks, which the owner's no-glitch rule rejects.

## Watch out
Test models must give the core a slack that follows their load (deadline less the blocks), or predictive recovery never fires (a fixed 150 us slack is below the margin).

In the code: core/engine/rust/src/budget.rs recover (measured, fits), Lane.worst; notes.rs worst_used; core/engine/tests/core_recovery.cpp chain.
