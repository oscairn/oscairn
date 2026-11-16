---
title: control-source failed on a slow runner because its load guard gave a note up
author: Jacob
date: 2026-11-16
branch: tsan-three
cost: { minutes: 139, tokens: 179521 }
id: l-0e84e1ee
scope: repo
status: active
src: agent
touches:
  - core/control/tests/control_source.cpp
  - core/engine/rust/src/notes.rs
---

## Decided
control-source sets OSCAIRN_LOAD_GUARD=off at the start of main, as parts.cpp, performance.cpp and performance_switch.cpp already do. It renders blocks as fast as they run and compares them sample for sample, which tests the lane, not load.

Why: with the guard on, a block that takes too long on a slow machine makes the engine core give a note up, and every sample after reads 0 (point 289, output 0: 0.000000). Under GCC ThreadSanitizer in Docker with 2 CPUs and 12 busy loops: 11 of 20 runs failed, and 13 of 20 at 0a684842, before the nine engine fixes; with the guard off 0 of 30. The CI figure '84913 wrong of 8004' is not a bug in the loop: bad counts samples over all outputs, checked counts points.

## Tried and ruled out
- The nine engine fixes between 0a684842 and 40003f71: the old commit fails as often.
- The ENVIRONMENT edit of control-shared-costs: it is inside its own set_tests_properties, no other test's environment changed.

## Watch out
- Any test that compares in-process plugin output for exact values needs the guard off, or it fails on a busy runner about when the machine is slow.

In the code: core/control/tests/control_source.cpp main; core/engine/rust/src/notes.rs (the budget's limits per block).
