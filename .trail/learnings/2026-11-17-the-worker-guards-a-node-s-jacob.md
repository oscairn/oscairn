---
title: "The worker guards a node's block only when another node of its worker mixes from it"
author: Jacob
date: 2026-11-17
branch: unit-findings-4
cost: { minutes: 1506, tokens: 2533079 }
id: l-bbd4a9b0
scope: repo
status: active
replaces: l-88a37719
src: agent
touches:
  - core/plugin-host/src/Worker.cpp
  - core/control/tests/worker_bench.cpp
---

## Decided
- Worker.cpp's guard() mirrors the engine's output guard only so that a follower mixed in the worker (RFC 0045 plugin groups, mix()) hears a source as the engine would. The engine guards what it reads from a worker itself. So render() now only marks a block unguarded, and mix() runs guard() once, at the first route from that source. A node that nothing in its worker mixes from is never scanned, which covers an instrument alone in its worker under plugin or chain placement.
- The scan is GCC vector-extension code: four lanes, two running maxima, the max of bits & 0x7fffffff (as the Rust core's magnitude). GCC 13 -O2 on aarch64 left the old fabs/compare loop scalar, and a 16-element array of maxima scalar too. In an arm64 container at -O2: 0.79 us old, 0.14 us new for 18 x 128 samples.
## Tried and ruled out
- Scanning only the connected channels: the engine fades the whole node when any of its channels is bad, so the mirror must read all of a consumed node's channels.
- The NaN case in plugin-groups as the safety net: it passes with the worker's guard removed. NaN never gets that far (the CLAP host stops the plugin), and a silent follower matches either way. control-group-guard instead mixes a finite runaway (two 9e5 sends into an effect) between two effects in one host: 200 bad blocks with the guard, 400 without it.
## Watch out
- On the Mac the change is invisible: clang already vectorized the old loop. Worker bench, 5 alternating runs: the nine-output instrument at 45-55 us a block both ways, ST Final's shape 4.2-4.35. The ~14 us saving is the unit's profile and is not measured there yet.
- When a node's first block is consumed after a stretch unconsumed, the mirror's last good sample is stale. That only matters if that very block is bad, and the value stays finite.
