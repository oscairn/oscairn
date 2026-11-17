---
title: The C++ CLAP host is deleted; what remains and how to change the host now
author: Jacob
date: 2026-11-17
branch: cpp-host-removal
cost: { minutes: 400, tokens: 2536832 }
id: l-12dbad76
scope: repo
status: active
replaces: l-83a7d2a4
src: agent
touches:
  - core/clap-host/rust/src/lib.rs
  - core/plugin-host/src/kick_clap.cpp
  - core/plugin-host/include/kick_clap/Host.h
  - core/engine/tests/clap_host_rust.cpp
---

## Decided
kick::clap::Host (Host.cpp), Internal.h, clap_host_cpp.cpp, clap-host-c, OSCAIRN_CLAP_HOST and the cpp-reference CI job are gone (2026-11-17). Discovery, the event types' code, the process's hardware and activation observer are the Rust host's (discovery.rs, isolated.rs, event.rs, services.rs); C++ callers use kick_clap/Host.h and Discovery.h, now in core/plugin-host/include, over rust_clap_* (plugin-host/src/kick_clap.cpp). The tests that built kick::clap::Host (kick-clap-host, host-policy, sequencer-*, midi-clock, delay-glide, hardware-plugin, handoff-bench, kick-clap-bench) load through oscairn::loadPlugin. A change to the host is a change to host.rs alone: replace the old rule that Host.cpp and host.rs move together. clap-host-rust-replay holds it to the C++ recording; a deliberate change records the Rust host's (clap-host-vectors) and the diff is reviewed.

## Watch out
Linux vectors were never recorded: the replay skips there (exit 77). Build commit 22c0484c (the last with the C++ host) on Linux x86_64 and aarch64 and run the clap-host-vectors target to make them. Still C++ in core/clap-host: the window island (Window.mm, WindowBridge.cpp, Window.h, Window.cpp, bridge.h), meant to become one window.mm with kick_clap/window.h (prepared, not applied), and the test fixtures. The 62 rust_clap_* host exports all serve ClapHost (plugin_host.cpp); they go when the worker and WorkerHost are Rust.
