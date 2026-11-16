---
title: Rust CLAP host held 300 KB more per instance than the C++ host
author: Jacob
date: 2026-11-16
branch: rust-host-memory
cost: { minutes: 595, tokens: 920025 }
id: l-19fe3544
scope: repo
status: active
src: agent
touches:
  - core/clap-host/rust/src/host.rs
---

## Decided
The Rust host made its event arrays (ordered, chunk_events, carry_events, block_output, voice_notes) as `Box::new([x; N])`, which writes every byte. They now come zeroed from the allocator (`zeroed_box`), so only pages a block uses become resident. Declared memory (`oscairn.memory`) was never different: Host.cpp and host.rs mirror it line for line; the 408 vs 411 MiB for TSO Carol Bells was measured worker memory.

## Tried and ruled out
Mirror bug in declaration or rounding: none. Extra threads: none. Cause is per Host, so it grows with instances.

## Watch out
Measured per worker (probe of N fixture instances, one worker each): C++ 4986 KiB, Rust before 5359, after 4939 (Linux arm64 container); macOS 8883, 9177, 8780. A new fixed array in Host costs every instance.

In the code: core/clap-host/rust/src/host.rs `zeroed_box`, `Host::create`.
