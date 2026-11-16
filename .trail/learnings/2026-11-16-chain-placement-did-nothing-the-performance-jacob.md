---
title: "chain placement did nothing: the Performance open path loaded plugins outside any placement group"
author: Jacob
date: 2026-11-16
branch: unit-findings
cost: { minutes: 60, tokens: 991381 }
id: l-624d0f7b
scope: repo
status: active
src: agent
touches:
  - core/control/src/performance.cpp
  - core/plugin-host/src/WorkerHost.cpp
---

## Decided
- `OSCAIRN_WORKERS=chain` (the profile's compute.workers) placed nothing on the unit: 22 processes for Sick tines, 9 for ST Final, as under `plugin`. The cause was not the start script or the parsing (the env var reaches policy() in plugin_host.cpp). Session::loadPerformance reads a Performance in Session::read (performance.cpp), which loads each plugin into the next graph, and only the import path (addNodes) set a PlacementScope. With an empty group key loadPluginInWorker gives `chain` a worker of its own.
- Both paths now take their groups from documentPlacement (performance.cpp, from the document's audio wires) and load each node under its PlacementScope. Test: control-chain-placement (3 parts of an instrument and 3 inserts through a Session save and open: 12 processes before, 6 after; the -plugin variant stays at 12).

## Tried and ruled out
- The start script and profile: OSCAIRN_WORKERS is passed through unchanged. No other source (profile JSON, document) sets the policy; the environment is the only one.
- plugin-groups-rest-* did not catch it: they build the Graph by hand under a PlacementScope.

## Watch out
- Instances taken over from the previous Performance (kept warm, playing) stay in the process they were loaded in; only fresh loads follow the placement.
- pool never showed the gap: it picks a host by cost even with an empty group.

In the code: Session::read in core/control/src/performance.cpp; loadPluginInWorker in core/plugin-host/src/WorkerHost.cpp.
