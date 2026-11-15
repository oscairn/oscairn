---
title: "The graph's audio plane moves to Rust; the graph and worker ports go on"
author: Jacob
date: 2026-11-15
branch: rust-graph
cost: { minutes: 28, tokens: 35175 }
id: l-86707b82
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/audio.rs
  - core/engine/src/graph.cpp
  - core/engine/src/engine_core.h
  - core/engine/tests/audio_diff.cpp
---

## Decided
- The owner reversed the call to leave the graph and the plugin worker in C++ (2026-11-14): "keep working on the rust graph and worker experiments, it still seems like rust would be beneficial even if code is slightly more", and extra time writing code is not a concern. The worker line under "ruled out" in the engine-core learning no longer holds; the worker is stage 2, with every feature the C++ worker has.
- Stage 1: a plan's audio (buffers, the audio connections with live gains and a played change's ramps, the output guard, meters, quiet/asleep/age) is the engine core's oscairn_audio (engine_core.h audio_*), Rust in core/engine/rust/src/audio.rs, its C++ reference in core/engine/src/audio_cpp.cpp (graph.cpp's code moved verbatim, old layout kept), selected by OSCAIRN_ENGINE_CORE as the rest of the core. graph.cpp keeps the step loop, plugin calls, events, harmony, taps and listeners.
- engine-audio-diff compares both bit for bit after every call on random plans and blocks (played changes mid-ramp, live gains, NaN/inf/past-bound samples, silence, unrendered blocks, plugins working in place); 15 one-token mutations of audio.rs are all caught.

## Tried and ruled out
- Expecting the layout (one buffer allocation in run order, connections resolved to buffer numbers) to cut memory traffic: cachegrind with the A53's caches (32 KB D1, 512 KB L2) on a 16-part, 35-step plan shows the same D1 misses a block (about 1,800) and no L2 misses for either; the audio buffers themselves (about 56 KB a block) are the traffic. The gain is instructions: 56k against 90k a block.
- A fused fold over (peak, bad) with f32::max: it does not vectorize on generic armv8-a (scalar fmaxnm); slower than g++'s C++.

## Watch out
- The guard, meters and quiet read each output once in the Rust: the channel's largest magnitude as an integer max over the bits with the sign masked (positive floats order as their bits; NaN and infinity sort above OUTPUT_BOUND), which vectorizes; the peak is re-measured only after a bad block's fade. Bit-identical to the C++'s three std::fmax passes.
- The mix no longer fuses multiply-adds (Rust never does; audio_cpp.cpp is built -ffp-contract=off to match), so mixed audio differs from older builds in the last bits. Not plugin-visible.
- Apple clang (at -O2 or -O3) builds the C++ reference much slower than g++ -O3 on Linux arm64 (10.6 against 4.4 us a block for the same plan); compare against g++ (the device toolchain's compiler) at the bundle's level.
