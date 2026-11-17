---
title: What still keeps the C++ CLAP host after its vectors are frozen
author: Jacob
date: 2026-11-17
branch: cpp-host-removal
cost: { minutes: 90, tokens: 2360774 }
id: l-83a7d2a4
scope: repo
status: active
replaces: l-aba7b4c8
src: agent
touches:
  - core/clap-host/src/Host.cpp
  - core/clap-host/include/kick_clap/Host.h
  - core/plugin-host/src/clap_host_cpp.cpp
  - core/CMakeLists.txt
  - ".github/workflows/ci.yml"
---

## Decided
The Rust CLAP host is the default since 2026-11-16 (owner, after the keyboard comparison: 87 Performances open the same, levels equal, memory level, cost and lost blocks better, worker recovery the same, an hour at rest clean); the C++ host stays compiled as the reference, and CI's cpp-reference-host job builds -DOSCAIRN_CLAP_HOST=cpp. Its outputs are frozen for Darwin-arm64 (core/clap-host/tests/vectors, clap-host-rust-replay); Linux vectors are not committed yet (CI uploads them as clap-host-vectors-linux).

Deleting Host.cpp's Host class (about 1,550 of its 1,731 lines), Host.h's member declarations (about 155) and clap_host_cpp.cpp (304) was put off on 2026-11-16 because code outside the C seam still constructs kick::clap::Host and must move to loadPlugin (the Rust host) first, about 400 to 600 changed lines: HostTests.cpp's conformance half (114 call sites; byte-vector state, latency(), currentMemory(), unload/faulted/guiOpen, none of which clap_host.h offers), the installed kick-clap-bench (tools/Benchmark.cpp and its two tests), handoff_bench.cpp, and the tests host-policy, sequencer-tracks, sequencer-params, sequencer-timing, midi-clock, delay-glide and hardware-plugin. A half-deleted host that still needs both is worse than waiting.

## Watch out
What stays C++ in core/clap-host after the deletion: Host.h's types and constants (Event, SysexPool, TransportWatch, Statistics, Descriptor, Parameter, Port, AssetResolver, HardwareAccess, maxEvents and the spaces: the engine, control, LV2 host and worker use them, so kick::clap::Host can shrink to the constants and the statics setHardware/observeActivations), Event's and TransportWatch's code (about 140 lines of Host.cpp, to an Event.cpp), the hardware and activation bridge at Host.cpp's end (to Bridge.cpp), Internal.h (Discovery's Library and PreparedInstrument's Queue), Discovery.cpp (scan helper, preset index, which bridge.h's kick_clap_presets serves the Rust host), Window.mm/Window.cpp/WindowBridge.cpp (plugin editor windows through kick_clap_window_*), and the fixtures. Order: move the users to loadPlugin, move Event/TransportWatch and the statics out, record Linux vectors, then delete the class, clap_host_cpp.cpp, clap-host-c, OSCAIRN_CLAP_HOST and the CI job together, after the device build.
