---
title: "Two events every block to every worker: the beat and a part's harmony, sent whether the plugin asked or not"
author: Jacob
date: 2026-11-16
branch: unit-findings
cost: { minutes: 60, tokens: 994930 }
id: l-cd237e42
scope: repo
status: active
src: agent
touches:
  - core/plugin-host/src/WorkerHost.cpp
  - core/engine/rust/src/run.rs
  - core/clap-host/src/Host.cpp
---

## Decided
- The two input events each worker counted per block (timing-input-events, night J1) are oscairn.beat, which both cores key into every step at sample 0 (run.rs prepare, run_cpp.cpp), and the harmony of the part a step is in, also at sample 0 every block (Session::harmonies puts every node of a part's chain in it). A plugin's host drops them on arrival unless the plugin asked (wantsBeat, wantsHarmony, event-registry) or the harmony is unchanged, so the cost was copying them through the shared block and the worker parsing them.
- WorkerHost::start no longer copies them for a plugin that did not ask. The worker says which it asked for in its description (PluginHost::wantsEvents, Host::wantsEvents in both CLAP hosts, the seam wants_events). The cores are untouched, so their differential tests stand. Test worker-timing-events: an invalid beat and harmony reaches a host (dropped events) only for the follower fixture. Measured on a Mac, 25 workers: about 2 us of 142 us a block of the engine thread, inside the noise.

## Tried and ruled out
- Gating the beat in both cores per step: the same effect for a worker, an engine_core.h and bindings.rs change, and the cores' own routing of two events is small.

## Watch out
- A plugin that asks for the events after it is loaded would not get them from a worker; the description is read at load. The harmony a wanting plugin already drops when unchanged is still sent every block.

In the code: WorkerHost::start, Worker.cpp describe, Host::wantsEvents.
