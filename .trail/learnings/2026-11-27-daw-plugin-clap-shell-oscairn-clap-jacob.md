---
title: "DAW plugin CLAP shell: Oscairn.clap over the core, slices, state while loading, tail infinite"
author: Jacob
date: 2026-11-27
branch: daw-clap
cost: { minutes: 29, tokens: 31879 }
id: l-a3dc8083
scope: repo
status: active
src: agent
touches:
  - hosts/plugin-core/plugin_core.cpp
---

## Decided
- Oscairn.clap (hosts/plugin-clap, id org.oscairn.oscairn) is one Instance per clap_plugin, made at init with Config.maxFrames 1024: the core's block size is fixed at construction, CLAP only gives max frames at activate, so process() plays a longer host block in slices of 1024 (events rebased per slice). Notes (note on/off/choke) and CLAP_EVENT_MIDI go to the core as MIDI by channel; note ports prefer the MIDI dialect. Latency 0, tail infinite (UINT32_MAX: the core cannot say how long plugins ring on).
- Main thread never waits for a load: load_state is setState (returns at once); Instance::activate now posts instead of waiting while a load is in progress (hosts often load state, then activate) and process() plays silence until the load is done; save_state while loading writes the bytes that load came from (state() would queue behind the load). init waits for the Session to start and destroy for a load in progress to end: not fixed.
- Temporary pick without a GUI: env OSCAIRN_CLAP_PERFORMANCE (library ID) at init, OSCAIRN_BUNDLED_PLUGINS for the plugin folder. Remove when the editor (#304) lands.
- The module exports only clap_entry (macOS -exported_symbol, Linux --exclude-libs ALL) so the engine's symbols stay out of the DAW.
## Watch out
- The test host links oscairn_control and dlopens the .clap: both hold the host's window ObjC classes, macOS prints a duplicate-class warning (harmless here). forbidAllocation in the test host does not reach the module's own operator new on macOS (two-level namespace); the core's own test covers allocation-free process(). The shell's process() itself allocates nothing by construction (fixed arrays).
- clap-validator was not installed, so it was not run.
