---
title: "One relaxed() for the timing tests, read at run time; CMake no longer reads CI"
author: Jacob
date: 2026-11-17
branch: flaky-round2
cost: { minutes: 50, tokens: 2256770 }
id: l-23000211
scope: repo
status: active
src: agent
touches:
  - core/test-support/relaxed.h
  - core/CMakeLists.txt
  - docs/developer-guide.md
  - core/engine/tests/worker_failures.cpp
---

## Decided
core/test-support/relaxed.h is the single relaxed(): CI set when the test runs, or a sanitizer build. The eight tests with a timing flag and engine-pump now take timing = no --no-timing argument and not relaxed(); CMake passes --no-timing for sanitizer builds only (core/CMakeLists.txt, hosts/engine/CMakeLists.txt no longer read CI at configure time). The rule and what each test still asserts is in docs/developer-guide.md, Timing tests.

Why: --no-timing was fixed at configure time from CI while four tests had their own relaxed() copies reading it at run time, so one build could be strict in one test and relaxed in another.

## Tried and ruled out
- worker-failures: block thread CPU time against wall time as the premise instead of the context-switch counter. A legitimate wait for the slow plugin (wall 1.4 to 1.8 ms) shows CPU 0 to 200 us, because the engine sleeps while it waits, so CPU against wall cannot tell a preemption from that wait. The counter stays; it is process-wide.

## Watch out
- Tests other than the six in the doc's table skip their timing assertions whole when relaxed; they have no premise of their own yet.
