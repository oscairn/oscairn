---
title: "The CLAP host's C seam sits under ClapHost, not under PluginHost"
author: Jacob
date: 2026-11-15
branch: rust-host-1
cost: { minutes: 13, tokens: 5320 }
id: l-5764ca0c
scope: repo
status: active
src: agent
touches:
  - "core/plugin-host/src/plugin_host.cpp#ClapHost"
---

decided: the Rust host's C interface (clap_host.h, cpp_clap_* now, rust_clap_* next) replaces kick::clap::Host plus ClapHost's state-file bodies; ClapHost stays the one C++ PluginHost facade and caches descriptor/parameters/ports, refreshed when clap_revision changes (service, reconfigure, loadState, loadPreset), always before rethrowing a failure since a failed restart clears them. Why: nothing in Rust calls a C++ PluginHost except run.rs, which already goes through oscairn_plugins, and PluginHost has eight C++ implementers (LV2, WorkerHost, SoundContext, PreparedInstrument, Unavailable, test doubles) and consumers in Graph, Session, parts, performance, budget, undo; a PluginHost vtable (the rust-graph.md handoff's sub-stage 1) would convert every one for no Rust caller. Worker.cpp already reaches the host through loadPlugin, so the same seam serves the worker. ruled out: carrying state as bytes across the C interface (64 MB states copied twice; directories instead, StateFiles moves with the host); a PluginHost-wide vtable. watch out: failures cross as kind + message (invalid_argument, logic_error, runtime_error) because json_rpc and Worker.cpp catch invalid_argument apart; filesystem_error arrives as runtime_error with the same what(), and nothing catches it by type. Preset URIs stay in plugin_host.cpp: scanPluginPresets makes them too.
