---
title: A change to Host.cpp needs the same change in the Rust CLAP host
author: Jacob
date: 2026-11-15
branch: rust-host-2
cost: { minutes: 110, tokens: 135119 }
id: l-8466cc0e
scope: repo
status: active
src: agent
touches:
  - core/clap-host/src/Host.cpp
  - core/clap-host/rust/src/host.rs
  - core/engine/tests/recording_fixture.cpp
---

watch out: until the C++ reference is deleted, core/clap-host/src/Host.cpp and core/clap-host/rust/src/host.rs are two copies of one behavior. main's 5af436fe/746ff473 (the beat and harmony alone wake no plugin asleep) changed only Host.cpp; under OSCAIRN_CLAP_HOST=rust worker-sleep then failed, while clap-host-rust passed, because the recording fixture never slept through a beat. decided: port such changes to host.rs in the same commit, and make the recording fixture exercise the new path so clap-host-rust compares it (here: the recorder now sleeps through blocks bringing only the beat and harmony). Also run the suites with OSCAIRN_CLAP_HOST=rust when Host.cpp changes.
