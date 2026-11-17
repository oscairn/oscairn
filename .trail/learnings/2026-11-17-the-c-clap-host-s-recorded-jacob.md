---
title: "The C++ CLAP host's recorded vectors: fixtures only, one file per target"
author: Jacob
date: 2026-11-17
branch: cpp-host-removal
cost: { minutes: 150, tokens: 2289418 }
id: l-bc1769ef
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/clap_host_rust.cpp
  - core/engine/tests/recording_fixture.cpp
  - core/CMakeLists.txt
---

## Decided
clap-host-rust-replay replays the Rust host against core/clap-host/tests/vectors/<CMAKE_SYSTEM_NAME>-<CMAKE_SYSTEM_PROCESSOR>.vectors, recorded from the C++ host (cmake --build BUILD --target clap-host-vectors, then reconfigure: CMake checks for the file at configure time; with none it records live first, as before). Only the test's fixtures are recorded (recorder, kick-clap-fixture-normal, voice fixture); clap-host-rust keeps Oscairn's plugins live while the C++ host exists. Items over 64 bytes are kept as an FNV-1a digest; audio other than the recorder's as size, nonzero count, sum and peak compared within 1e-4. Darwin-arm64: 102,345 items, 1.75 MB.

## Why
Oscairn's plugins bump their version with every binary change and their state, parameters and audio move with plugin work, so a recording of them breaks for reasons that are not the host's. Failure messages (libc++ vs libstdc++ filesystem_error text, which the Rust host copies per system) make a recording per target unavoidable. The recorder no longer folds the host's version string (Oscairn's release) into its init hash, and the test's input audio is an exact sawtooth instead of std::sin: both would change a recording without any host change.

## Tried and ruled out
Raw items with a full label each: about 5 MB. Bit-exact audio for the voice fixture and Oscairn's plugins: they call libm (sin, exp, pow), whose last bits vary between OS releases.

## Watch out
A recording made with a mutation of host.rs present is still the C++ host's (record runs only cpp_clap_*). A CLAP-to-MIDI velocity scaled by 126 instead of 127 passed both live and replay until the translated note's velocity was 0.8 (0.7 gives 88 either way): a mutation that passes means a scenario too weak, not a safe change. Linux vectors are not committed yet: the Linux CI job records them as the artifact clap-host-vectors-linux.
