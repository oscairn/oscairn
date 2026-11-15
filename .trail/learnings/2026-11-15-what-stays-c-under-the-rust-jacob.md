---
title: What stays C++ under the Rust CLAP host is reached through kick_clap/bridge.h
author: Jacob
date: 2026-11-15
branch: rust-host-2
cost: { minutes: 75, tokens: 70565 }
id: l-48469178
scope: repo
status: active
src: agent
touches:
  - core/clap-host/include/kick_clap/bridge.h
  - core/clap-host/src/Bridge.cpp
  - core/plugin-host/src/clap_host.h
---

decided: the process-wide hardware access and activation observer keep being set on kick::clap::Host (hardware service, Worker.cpp unchanged), and the Rust host reads them through C calls in kick_clap/bridge.h (kick_clap_hardware_*, kick_clap_activating), as it reaches preset indexing and the host's windows there. ruled out: set_hardware/observe_activations in clap_host.h with C tables, which would need a HardwareAccess adapter in the reference and make the hardware library depend on plugin-host, and a C++ reference fed its own static through an adapter recurses. Assets are different: they are per host, so oscairn_clap_assets is a C table, made from an AssetResolver by oscairn_clap_assets_of (Host.cpp's buffer checks and messages inside it), and copied by value into each host (an effect context must not keep a pointer into its parent). watch out: comparing hosts on assets needs that real table: a simpler fake skips Host.cpp's digest check on resolve_sample and the hosts differ.
