---
title: "voice-budget SDK v3 mirrored; the scenario's TSAN flake was the piano's own lane"
author: Jacob
date: 2026-11-20
branch: voice-importance-3
cost: { minutes: 2180, tokens: 5712737 }
id: l-5a9a0d29
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/voice_scenario.cpp
  - core/clap-ext/include/oscairn-clap/voice-budget-plugin.hpp
  - core/clap-host/rust/src/lib.rs
  - core/engine/rust/src/bindings.rs
---

## Decided
- voice-importance-scenario's TSAN flake (no sanitizer report) was 'pool, two processes: the piano, on another lane, loses nothing for the lead's'. Under ThreadSanitizer a fixture piano voice costs about 30 us a block, not 15, so 30 notes plus rest passed the piano's own lane limit (800 us) and its lane faded 10-11 voices (NoteCost.stolen) at blocks 20-99, before the lead struck a key, with no lost block: the lost-block premise let the claim through. The claim is now also premised on pianoBefore==30 (the piano kept its 30 notes until the lead played). 20 TSAN runs, 0 failures; on the Mac the claim is still checked in all three apart placements.
- Mirroring an SDK header change also needs two Rust hashes: core/clap-host/rust/src/lib.rs extension_headers_unchanged (every .h in clap-ext) and core/engine/rust/src/bindings.rs MADE_FROM[1] (voice-budget.h). Bindgen emits no comments, so a comment-only header change moves only the hashes.
- SDK v3: next() takes victim()'s order across contexts (tier before priority), matching the host's key; an engine that gives a note up only whole counts notes (the VA's unit is a note, weight = unison width).
## Ruled out
- A premise on the piano's measured cost (rest + 30 x perNote <= laneLimit at the end): in a failing run perNote read 17 us at the end after the lane had shed at 30.
## Watch out
- On this Mac (2026-11-20), desktop-shell (timeout 120 s), desktop-gaps (timeout 400 s, Local RPC timed out), ui-waits (the app shows A/B 117-192 ms after load), worker-failures (leak fixture got "" not 'too much memory') and control-shared-costs (ratio 2.0-2.7) fail alone on clean main b7a5e3fc too: not voice-importance's.
