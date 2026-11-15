---
title: "Plugin groups need placement by chain: three hosts by cost formed no group"
author: Jacob
date: 2026-11-15
branch: worker-floor
cost: { minutes: 150, tokens: 1513 }
id: l-d83fee17
scope: repo
status: active
src: agent
touches:
  - core/engine/src/graph.cpp
  - core/plugin-host/src/WorkerHost.cpp
  - core/plugin-host/src/Worker.cpp
---

## Decided
- Plugin groups (graph.cpp compile, WorkerHost::startGrouped, Worker.cpp mix): a node joins its audio sources' group when they share a worker process, all its audio comes from that one group, and its notes and lanes come from earlier waves. It then renders in the same round trip, its input mixed in the worker. The run cores are untouched. Exact against ungrouped (plugin-groups-rest-*, both cores, three rest modes) because the worker mixes with the core's own formula compiled -ffp-contract=off and keeps a mirror of the engine's output guard. A follower is never rested while a source was handed that block.
- A played change (commit(true)) forms no groups: its gain ramps are the engine core's state, which the worker cannot see.

## Tried and ruled out
- Pool placement by cost as the grouping policy: with three hosts every chain was scattered and 0 of 22 nodes grouped (oscairn-group-bench). One host grouped 21 of 22 (block p50 215-229 us against 644-698 ungrouped in the container). The `chain` policy needs placement at load by part, which Graph::add cannot do because it knows no part.
- Regenerating bindings.rs with core/engine/rust/bindgen on this Mac: its libclang emits a different file and rewrites core/clap-host/rust/src/bindings.rs too. Edit bindings.rs by hand for a worker_shared.h change and update MADE_FROM (FNV-1a of the header).

## Watch out
- Instruments added with Graph::add render through PreparedInstrument (not handed over), so they never head a group: the first effect after them does.
- worker-wake, worker-failures and plugin-hosts fail their timing checks in the arm64 container on main too. Compare against a main build before blaming a change.
