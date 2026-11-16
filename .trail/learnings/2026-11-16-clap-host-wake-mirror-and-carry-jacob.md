---
title: "CLAP host: wake mirror and carry count atomic, wrong-thread main() counts an error"
author: Jacob
date: 2026-11-16
branch: host-races
cost: { minutes: 188, tokens: 321555 }
id: l-3d6d2a8e
scope: repo
status: active
src: agent
touches:
  - core/clap-host/src/Host.cpp
  - core/clap-host/rust/src/host.rs
  - core/clap-host/rust/src/more.rs
  - core/clap-host/tests/HostTests.cpp
---

## Decided
The three races the Rust handle refactor found are fixed in both hosts alike. The wake mirror is an atomic pointer (seq_cst, so a wake and a mirror set cannot both miss each other; a plain load on the audio path). carry_count is an atomic written relaxed under the gate and read relaxed by restful(): a stale read rests a worker one block more or keeps it awake one block more, and carried events are never dropped. A control call on the wrong thread counts one in errors and writes no text; message stays the main thread's.

## Tried and ruled out
Setting the mirror before the node is published: the plugin's own threads and detach()'s mirrorWake(nullptr) still race with it. A lock for the count: restful() runs on the audio thread. Keeping the text by flagging it for error(): a flag set long ago would replace a later message.

## Watch out
Under TSAN the test in HostTests.cpp (a thread renders and asks restful(), one enqueues, main calls mirrorWake, saveState and loadState) reported all three on the old code and is silent now. Owned::racy remains only for gui_floating.

In the code: Impl::wakeMirror and carryCount in Host.cpp; wake_mirror and carry_count in host.rs.
