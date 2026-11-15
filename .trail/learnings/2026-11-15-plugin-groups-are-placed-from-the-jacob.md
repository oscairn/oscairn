---
title: "Plugin groups are placed from the Performance document before loading; the group, not the plugin, is the unit"
author: Jacob
date: 2026-11-15
branch: worker-floor
cost: { minutes: 120, tokens: 62885 }
id: l-c887a3e7
scope: repo
status: active
src: agent
touches:
  - core/control/src/performance.cpp
  - core/plugin-host/src/plugin_host.cpp
  - core/plugin-host/src/WorkerHost.cpp
---

## Decided
- Placement is decided from the document before any plugin loads. Session::addNodes runs placementGroups over the document's audio wires (the same rule the plan compiler groups by) and loads each plugin under a thread-local PlacementScope. Moving nodes between hosts after the plan would reload every plugin, which is slower and audible. Performance open times did not change (control-performance-switch-workers).
- compute.workers / OSCAIRN_WORKERS selects plugin, chain or pool. chain gives each group its own process, which goes away with its last node. pool never splits a group, and an instrument (an alone group) takes an empty host while other hosts remain. compute.pluginHosts alone still means pool. The own-host list, by ID or "publisher.*", wins over every policy.
- OSCAIRN_ENGINE_AUDIO_CORES and OSCAIRN_WORKER_AUDIO_CORES each replace OSCAIRN_AUDIO_THREAD_CORES for their side. Under chain or pool, each worker process gets one core of the workers' list, round-robin; the slot is the worker's 4th argv.

## Tried and ruled out
- Testing sound contexts with no sound of their own (std::nullopt): they share the Default sound and its Output bus, so all five parts played on one bus. Give each part its own prepared sound.
- An unpaced kill test with a 200 ms allowance: a dead worker then costs 200 ms a block and looks like every part going silent. Pace it with a callback deadline, as a device does.

## Watch out
- Pool hosts are resident and keep their flags. Pool::pick clears `alone` on empty hosts and uses only the first `count` hosts, so a policy change within one process (as in the tests) does not inherit old hosts.
- Instruments added through Graph::add render through PreparedInstrument and are never handed over, so grouping barely helps a shape whose instruments are in that path (the two-instrument bench).
