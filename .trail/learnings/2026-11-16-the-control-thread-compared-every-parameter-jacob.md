---
title: "The control thread compared every parameter every tick; now on a host's value revision"
author: Jacob
date: 2026-11-16
branch: unit-findings-2
cost: { minutes: 1140, tokens: 1774239 }
id: l-505b3d8d
scope: repo
status: active
src: agent
touches:
  - "core/control/src/session.cpp#Session::service"
  - core/plugin-host/include/oscairn/plugin_host.h
  - core/plugin-host/src/WorkerHost.cpp
  - core/plugin-host/src/Worker.cpp
  - core/engine/include/oscairn/sound_context.h
---

decided: Session::service compared parameterValue of every parameter of every node each 5 ms tick against observed_, to publish values changed with no output event (params.changed, source plugin). Nothing else reads observed_: undo, scenes and learned controls read the hosts directly; plugin output events are published from popOutput already. A host's cached values change without an event only when it reads them again: CLAP_PARAM_RESCAN_VALUES and a preset load in both CLAP hosts (counted as valueRescans, C seam clap_value_rescans), a sound shown on a SoundContext (selectSound, setValues), a worker's state/preset/snapshot/sound requests and restart. PluginHost::valueRevision says so; a worker forwards its host's as a byte in the Service answer (2: cannot tell). Session compares a node's values only when that moves, at its first tick, after dropped output events, and every tick for a host that cannot say (LV2, PreparedInstrument). A value the host clamped or stepped from a set is published in sendParameters, as the scan did a tick later. No safety-net reconciliation: the hosts' write sites to their value caches are the closed set above. Cost (service_cost, Mac, 25 workers): 6,125 parameters 441 to 98 us a tick, 1,225,000 to 0 values compared a second. ruled out: a field in the worker's shared block (layout and binding change) for a plugin's own changes; they come as events with the worker's service. watch out: in a worker a plugin's own value change reaches clients with the next service, up to 100 ms after half a second of quiet (the scan saw the shared values within 5 ms). PreparedInstrument still scans every tick (default unreported).
