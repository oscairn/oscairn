---
title: The Rust CLAP host is the default; what must hold before the C++ host is deleted
author: Jacob
date: 2026-11-16
branch: rust-host-default
cost: { minutes: 1073, tokens: 1661652 }
id: l-aba7b4c8
scope: repo
status: active
src: agent
touches:
  - core/CMakeLists.txt
  - ".github/workflows/ci.yml"
---

decided: OSCAIRN_CLAP_HOST defaults to rust since this commit (owner, 2026-11-16, after the keyboard comparison: 87 Performances open the same, levels equal, memory level, cost and lost blocks better, worker recovery the same, an hour at rest clean). The C++ host (kick::clap::Host) stays compiled as the reference; CI has a cpp-reference-host job that builds -DOSCAIRN_CLAP_HOST=cpp and runs the host, worker, control and engine tests. The Rust host opens plugin editor windows through the bridge to kick_clap's C++ window code (kick_clap_window_*), so that code stays. watch out: before the C++ host is deleted, recorded transcripts must replace what clap-host-rust and clap-host-rust-replay compare against it, the device (Buildroot) build and the keyboard must confirm the default, and the constants and observeActivations the engine and worker still take from kick::clap::Host (Host.h) must move out first. ruled out: deleting or weakening any C++ host code or test in the flip.
