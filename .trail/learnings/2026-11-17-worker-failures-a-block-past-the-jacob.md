---
title: "worker-failures: a block past the period while the machine took the processor is the machine's"
author: Jacob
date: 2026-11-17
branch: flaky-three
cost: { minutes: 40, tokens: 2044027 }
id: l-3a41c5f1
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/worker_failures.cpp
---

## Decided
worker_failures.cpp counts a block past the period as an overrun only when the process had no involuntary context switch during it (getrusage ru_nivcsw before and after graph.process); such blocks are counted apart and printed.

Why: the bound is the period (2667 us at 128 frames, 48 kHz). The too-slow plugin's block takes about 2.0 ms and the engine waits for it while the start's grace lasts, so the worst block is 2380 to 2400 us alone, 270 us under the bound; a loaded Mac pushed one to 2678. There is nothing in the engine's own figures to measure instead: the wait is real. A wait the engine chose and a preemption of the paced thread look the same in wall time, but only the second shows in the context switches.

## Tried and ruled out
- Not reproduced: 0 of 60 under ctest -j3 beside, 0 of 30 at background QoS with 40 busy loops. The 3 of 30 seen earlier was not seen again, so no before and after rate; the change is unproven under load.
- Measuring the failing worker's reported block time: it bounds the wait only loosely, not to the period.

## Watch out
- ru_nivcsw is process-wide: a switch of another thread in the test process hides a real overrun in that block. Unverified how often the supervisor threads are switched out.
