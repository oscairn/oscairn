---
title: "Rust for the UI path and readers of untrusted files; the engine, worker and graph stay C++"
author: Jacob
date: 2026-11-14
branch: main
cost: { minutes: 30, tokens: 84852 }
id: l-784bd2b8
scope: repo
status: active
src: agent
touches:
  - AGENTS.md
  - core/control/CMakeLists.txt
  - core/control/tests/tick_signal.cpp
  - core/plugin-host/include/oscairn/plugin_host.h
  - core/plugin-host/src/worker_main.cpp
  - devices/roland-fantom0/bundle/oscairn-fantom-start
  - devices/roland-fantom0/tests/fantom_start.sh
  - devices/roland-fantom0/tools/panel_holder.cpp
  - docs/fantom-0.md
  - hosts/engine/main.cpp
---

## Decided
- New UI work and new readers of untrusted files (archives, sample packs) are written in Rust. The engine, the plugin worker and the audio graph stay C++. The owner decided this on 2026-11-14 from three experiments.
- UI: the Setlist prototype in Rust with Slint, drawn straight to the display with no X server, gave 0 audio misses in a 10 s scroll on the FANTOM against about 650 for the JUCE app under X, at 57% of a core with a 30 frames/s cap.
- Readers: a Rust port of readPackage used 2 to 3 times less memory and refused five malformed archives the C++ reader accepts on read and only fails on at extraction.

## Tried and ruled out
- Porting the plugin worker to Rust: same block round trip as C++ (16 to 32 us), 2,852 lines for a subset that passes 36 of the 69 worker tests against about 2,600 lines of C++ covering everything, and most of its memory gain came from not linking AppKit and from lazy buffers, which C++ can do too. Speed was never the problem, so an efficiency pass would not change it.
- A shadow buffer in the Slint software renderer to avoid blending in video memory: frames got slower (24 to 35 ms against 18 to 20) and audio misses came back. The CPU cost is the renderer's own work, proportional to frames drawn.

## Watch out
- The audio graph was never ported, so the engine verdict rests on the worker port alone. It calls every plugin host, so a port means a C++/Rust boundary there or rewriting the engines first.

In the code: branches experiment/rust-setlist, experiment/rust-package (experiments/rust-package/), experiment/rust-worker (experiments/rust-worker/); the C++ sides are core/control/src/package.cpp, core/plugin-host/src/Worker.cpp and WorkerHost.cpp, core/engine/src/graph.cpp; cmake/SamplePack.cmake already brings a Rust crate into the build.
