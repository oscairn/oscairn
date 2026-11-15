---
title: "engine-run-diff in Debug: explicit fma and an opaque pow base"
author: Jacob
date: 2026-11-15
branch: worktree-agent-a12534ec83ca271d3
cost: { minutes: 135, tokens: 166494 }
id: l-3fe41bda
scope: repo
status: active
src: agent
touches:
  - core/engine/src/run_cpp.cpp
  - core/engine/src/graph.cpp
  - core/engine/include/oscairn/graph.h
  - core/engine/rust/src/run.rs
  - core/control/src/parts.cpp
  - core/CMakeLists.txt
---

## Decided
- The Rust engine core is always built release (cargo --release), the C++ follows the CMake build type, so any arithmetic the optimizer may rewrite breaks engine-run-diff in Debug. Fused multiply-adds are now named: std::fma in Graph::ControlCurve::point, the Db shape and run_cpp.cpp's minimum + x * range, matching Rust mul_add; run_cpp.cpp joins core_cpp, audio_cpp and recorder in the -ffp-contract=off list. parts.cpp's summing rule calls curveOf(...).point(...) instead of repeating start + (end - start) * shape.
- The Db curve's pow(10, y) is kept as pow in both cores by an opaque base (static volatile in graph.cpp, black_box in run.rs). Clang at -O1 and up turns pow(10, y) into exp10, which rounds differently; at -O0 the C++ calls pow while the release Rust got exp10, a 3-ulp gap that failed call 75 to 78 in Debug.

## Tried and ruled out
- Explicit fma alone: the Debug failure stayed (same value), so the fp-contract guess was not the whole cause; the differing values were SCurve plus Db paths, and Db's pow/exp10 was it.

## Watch out
- Db values can differ in the last bits from builds before this change (exp10 became pow) in release too.
