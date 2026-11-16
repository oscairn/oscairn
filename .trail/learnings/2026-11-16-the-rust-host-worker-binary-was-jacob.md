---
title: The Rust-host worker binary was large from unused staticlib code
author: Jacob
date: 2026-11-16
branch: rust-host-rss
cost: { minutes: 702, tokens: 1058487 }
id: l-6a5acbb6
scope: repo
status: active
src: agent
touches:
  - core/CMakeLists.txt
---

## Decided
The worker linked without --gc-sections. The engine core's Rust staticlib (lto, codegen-units 1) is one object, so once the Rust CLAP host is referenced the whole of it and its std came in. Linking the worker with --gc-sections (-dead_strip on macOS) took the Linux aarch64 Rust worker from 1,534,089 to 963,524 bytes of text (stripped 1,577,864 to 987,864); the C++ worker barely moves (592 KB stripped). Same code kept, so the audio path is unchanged.

## Tried and ruled out
opt-level s/z (slows the hot path, not measured); panic=abort is already the release profile and nothing calls catch_unwind.

## Watch out
About 150 KB left is std's backtrace symbolizer (gimli, addr2line, miniz_oxide) behind the default panic hook; removing it needs build-std (nightly).
