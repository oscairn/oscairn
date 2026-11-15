---
title: Comparing the Rust CLAP host with the C++ one needs a recording plugin and a process each
author: Jacob
date: 2026-11-15
branch: rust-host-2
cost: { minutes: 30, tokens: 48670 }
id: l-c4777dfa
scope: repo
status: active
src: agent
---

decided: clap-host-rust runs cpp_clap_* and rust_clap_* in a forked process each and compares labeled transcripts, because a CLAP binary's entry init/deinit is once per process and each implementation keeps its own library registry (two in one process would init a binary twice). The kick fixtures and Oscairn plugins alone hid most host differences (steady time, transport header, MIDI/CLAP note translation, event validation): 6 of 12 one-token mutations survived until recording_fixture.cpp, which hashes every byte a block gives it into its audio and echoes its events. It found a real bug at once: events the host builds (set_parameter, gestures) must be zeroed first, padding included, as the C++ value-initializes them, since the plugin sees all 64 bytes. watch out: clap-sys 0.5.0 is CLAP 1.2.2 while Oscairn's headers are 1.2.10, so the host announces its own CLAP_VERSION; libc++'s filesystem_error for canonical ends with [""] and is part of the load failure message; Rust allocations bypass operator new, so the test counts malloc by defining it in the executable (the Rust staticlib binds to it). ruled out: AllocationGuard alone for Rust.
