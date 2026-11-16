---
title: control-shared-costs failed under load because it measured wall time in its own process
author: Jacob
date: 2026-11-16
branch: flaky-two
cost: { minutes: 405, tokens: 735366 }
id: l-c209dbfd
scope: repo
status: active
src: agent
touches:
  - core/control/tests/shared_costs.cpp
  - core/control/CMakeLists.txt
  - core/control/src/budget.cpp
  - core/clap-host/src/Host.cpp
---

## Decided
control-shared-costs runs its plugin in a worker (OSCAIRN_PLUGIN_WORKER, scan and budget set in CMake, as the engine runs plugins), so a block's cost is the worker thread's CPU time.

Why: with the plugin in the test's own process the cost is steady_clock wall time of the render call. Under load that is a stretched figure: median block of 24 voices 25 us alone, 35 us loaded, a few blocks of 5-80 ms. Session::measure takes each tick's share (at most one period) into a running mean weighted 1/n, so one period-capped sample is worth hundreds of ordinary ones. The same one-part setup measured again in one process read 0.00024 to 0.0009 per voice. 12 jobs x 30 runs beside 120 busy loops: 89 of 360 failed in process, 0 of 360 in a worker.

## Tried and ruled out
- A single stall at a moment: the figure was off in nearly every loaded run, not now and then.
- The engine core's cost (core/engine/rust/src/notes.rs): median of five plus a 2x guard, its per-note value held 0.8 to 1.4 us loaded against 0.6 to 1.05 alone.

## Watch out
- In-process hosts (the desktop app without a worker) still count wall time in Host.cpp totalUs; a spike is capped at a period only. control-estimates pins that a computing stall is counted, so a cap on samples would break it.

In the code: core/control/src/budget.cpp Session::measure; core/clap-host/src/Host.cpp (totalUs); core/plugin-host/src/Worker.cpp (cpuNs).
