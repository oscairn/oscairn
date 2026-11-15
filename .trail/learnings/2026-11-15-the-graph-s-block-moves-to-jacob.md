---
title: "The graph's block moves to Rust behind plugin callbacks; the reference is the old graph.cpp"
author: Jacob
date: 2026-11-15
branch: rust-graph-events
cost: { minutes: 1, tokens: 6642 }
id: l-005f999c
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/run.rs
  - core/engine/src/run_cpp.cpp
  - core/engine/src/graph.cpp
  - core/engine/src/engine_core.h
  - core/engine/tests/run_diff.cpp
---

## Decided
- Stage 1b: Graph::process from the beat on and beginBlock's plan change are the engine core's run (engine_core.h run_*): Rust in core/engine/rust/src/run.rs, the C++ reference run_cpp.cpp is graph.cpp's code moved with its PluginHost calls through oscairn_plugins (C function pointers; graph.cpp's table of one-line lambdas). graph.cpp keeps validation, the pause, the parameter queue, live gains, the beat and those lambdas.
- run_cpp.cpp is built with the default fp-contract, as graph.cpp was, so it is the old arithmetic; the Rust uses f64::mul_add exactly where clang fuses (a curve's point and Db shape, a lane's minimum + x * range), and engine-run-diff catches dropping any of them.
- Events stay CLAP's 64-byte events end to end: keys point at the plugins' own output lists, the merged block is handed to the plugin as is.

## Tried and ruled out
- Comparing the runs with budgets or voice-budget plugins under a budget, or blocks reported missed: the two notes cores differ there by design (RFC 0045 12a-12k are the Rust core's alone), so engine-run-diff keeps the budget at 0 or -1, lane times under the deadline, no missed blocks; engine-core-diff and engine-core-recovery cover those rules.
- Two mutations engine-run-diff misses are equivalent: clamping a MODULATE amount to -1..1 before adding the control's origin (origin is in 0..1 and the sum is clamped), and clamping between a modulating path's curves (each curve clamps its input).

## Watch out
- On x86 without FMA the old C++ did not fuse, so param-target values there differ in the last bit from older builds. Apple clang arm64 is proven unchanged; the FANTOM's g++ (fp-contract=fast) is expected to fuse the same expressions but is unverified.
- core/control/src/parts.cpp evaluates Graph::ControlCurve::shape on the control thread for its summing rule; it stays C++ and fused, matching what the run computes.
