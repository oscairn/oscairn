---
title: "The engine thread's core moves to Rust, staged and proven identical; plugins stay C++"
author: Jacob
date: 2026-11-14
branch: main
cost: { minutes: 109, tokens: 197400 }
id: l-a6e0fd79
scope: repo
status: active
replaces: l-784bd2b8
src: agent
touches:
  - core/CMakeLists.txt
  - core/engine/src/graph.cpp
  - core/engine/tests/peak_fmax.cpp
  - core/control/CMakeLists.txt
  - core/control/tests/worker_timing.cpp
  - core/engine/tests/worker_extra_block.cpp
  - hosts/engine/CMakeLists.txt
  - hosts/engine/tests/pump.cpp
  - core/control/src/json_rpc.cpp
  - core/control/tests/json_rpc.py
---

## Decided
- The owner decided on 2026-11-15 that the engine thread's core (note routing, voice budget, load guard, the graph's own per-block work) moves to Rust, with the reworked no-glitch guard written there. UI work and readers of untrusted files are Rust too, as decided the day before. Plugins (engines, effects) stay C++.
- Why: on the FANTOM's Cortex-A53 the Rust core is level with the best C++ build (24,000 against 22,750 ns a block on a workload modeled on a real Performance, equal on a 16-part one), its logic has no unsafe, and the guard was about to be reworked anyway. The effort of writing a port is not counted as a cost here.
- Each stage must be proven bit-identical against the C++ on the same random event streams before it replaces it, and the C++/Rust boundary's cost measured on the device before porting further.

## Tried and ruled out
- Porting the plugin worker process to Rust: same speed, more code for a subset of features; not decided either way.
- A slot-table redesign (bitmaps, 32-byte slots) for the note routes: no gain in either language.

## Watch out
- The device bundle built the engine at -O2, where the max-of-abs loops in the graph's finish step do not vectorize; the same C++ was 2.4 times slower than at -O3. Check the optimization level before comparing anything.
- The plugin hosts the graph calls every block are C++, so that boundary is on the hottest path.

In the code: experiments/rust-engine-core/ on branch experiment/rust-engine-core-2 (the proven core, its differential test and the device benchmark); core/engine/src/NoteRoute.h and graph.cpp are the C++ it mirrors.
