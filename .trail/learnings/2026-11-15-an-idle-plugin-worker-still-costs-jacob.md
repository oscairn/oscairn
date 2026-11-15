---
title: "An idle plugin worker still costs about 35 microseconds a block on the FANTOM, because the engine calls every worker every block"
author: Jacob
date: 2026-11-15
branch: main
cost: { minutes: 45, tokens: 60697 }
id: l-880db675
scope: repo
status: active
src: agent
touches:
  - core/CMakeLists.txt
  - core/control/tests/recording.cpp
  - core/engine/rust/src/audio.rs
  - core/engine/rust/src/lib.rs
  - core/engine/src/audio_cpp.cpp
  - core/engine/src/engine_core.h
  - core/engine/src/graph.cpp
  - core/engine/tests/audio_diff.cpp
  - docs/panel-map.schema.json
  - core/control/include/oscairn/session.h
---

## Decided
- Measured on the FANTOM-07 (host e1ac1cf3, ST Final at rest, 10 s): process ran on 3756 of 3758 blocks for every worker. Idle cost per block: effect workers 34-38 us, VA 73, arpeggiator 73, Sampler 93. The effects themselves idle at 1.5 us warm and 9 us cold standalone, so about 35 us is the round trip to a worker process, and the rest is a plugin that does not report asleep when idle.
- The host fix is to not wake a worker whose plugin is asleep with no input, events, transport wake or flush, and to report called against asleep block counts. It waits for the Rust graph event stage because both change the per-block step loop.

## Tried and ruled out
- Blaming the effects' shared wrapper or their algorithms for the ~100 us idle readings: the standalone timing showed the algorithms sleep correctly once the effect returns the sleep status.

## Watch out
- The in-process host already skips a sleeping plugin's process call, so "the host honors sleep" is true and still does not save the round trip.
- Engine timing has no asleep-block counter yet, so this split cannot be read from a running engine.

In the code: `core/clap-host/src/Host.cpp` (the sleeping check in `Host::process`), `core/plugin-host/src/Worker.h` (`asleep`), the step loop in `core/engine/src/graph.cpp`.
