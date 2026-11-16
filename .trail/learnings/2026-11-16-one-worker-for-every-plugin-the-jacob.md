---
title: "One worker for every plugin: the lane's soft limit shed every sustained voice, so a part is left its last note"
author: Jacob
date: 2026-11-16
branch: unit-findings
cost: { minutes: 90, tokens: 36524 }
id: l-6ba2ea81
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/budget.rs
  - core/engine/src/NoteRoute.h
---

## Decided
- A lane is one worker process. With pool 1 all 22 Sick tines plugins are one lane: Budget::estimate sums every node's cost (the medians of their wall times) against 0.85 x the lane limit (0.8 x the allowance, 1067 us on the unit), where a worker each compares each alone, in parallel. ST Final in one host showed lane took 908 us at rest (9 nodes) against a host thread using 8.3 percent of a core (night J3, pool1). When the others alone pass the soft limit admit and measured fade every sustained note: 0 to 1 voices, no lost block.
- The soft fades (admit, measured, device) now leave each connection its last note (choose_soft in budget.rs, choose(lane,false,true) in NoteRoute.h, both cores). core_pool.cpp: three sounds, 21 effects at 40 us: 2 voices total before (one sound with none), 4 after; a worker each keeps 21.

## Tried and ruled out
- Not dividing a limit per plugin: nothing divides; the sum is real, as one pass of one thread must fit the allowance. Fewer voices than a worker each is correct where costs are real; none is not.

## Watch out
- Unverified on the unit: whether the wall-time medians overstate the host's CPU. Capture engine.timing guard limit/took per node under pool 1.

In the code: Budget::choose_soft, estimate in core/engine/rust/src/budget.rs; VoiceBudget::choose in core/engine/src/NoteRoute.h.
