---
title: "guard-deadline: none-given-up claim needs the machine not to have held two of four blocks to their deadline"
author: Jacob
date: 2026-11-17
branch: flaky-three
cost: { minutes: 40, tokens: 2115383 }
id: l-402b1dc3
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/guard.cpp
  - core/engine/rust/src/notes.rs
  - core/engine/rust/src/budget.rs
---

## Decided
guard.cpp late() times each block (play's took) and checks 'none while it keeps within the deadline' for the little-left and whole-time runs only when no two of four blocks took as long as the time they were given.

Why: the guard counts the callback's time as run out when slack <= low_slack in two of the last four blocks (notes.rs finish: slack bits; budget.rs deadline), slack being the wall time left at the end of the block. The runs inject a 1000 us (or 2.4 ms) deadline over blocks of about 250 us, so a runner that stalls the paced thread two times in four blocks makes a note given up the rule working. took >= time left is necessary for that, so with no such pair a given-up note is the product's fault and is still checked exactly.

## Tried and ruled out
- Reproducing the failure: 0 of 40 on this Mac at background QoS with 40 busy loops and ctest -j8 beside. The CI failure (1 given up with little left) is explained by the rule, not reproduced.
- A model clock: the engine core reads steady_clock itself, so the test cannot supply one.

## Watch out
- guard.cpp consults neither relaxed() nor --no-timing for this check; the --no-timing flag is chosen at CMake configure time from CI.
