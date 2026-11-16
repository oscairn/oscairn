---
title: "Rust CLAP host handle: shared Host only, Owned fields, three races kept"
author: Jacob
date: 2026-11-16
branch: rust-host-size
cost: { minutes: 141, tokens: 235918 }
id: l-86474516
scope: repo
status: active
src: agent
touches:
  - core/clap-host/rust/src/host.rs
  - core/clap-host/rust/src/lib.rs
  - core/clap-host/rust/src/more.rs
---

## Decided
The Rust CLAP host reaches its Host only through `H` (host.rs), a Copy pointer whose Deref gives `&Host`, never `&mut Host`: the audio thread and the control thread hold the host at once, so a whole-struct `&mut` would alias the other thread's atomics. Every field written after construction is an atomic, a queue, or an `Owned<T>` (UnsafeCell: `get`/`set`/`with`), grouped in the struct as Host.cpp's Impl splits them: main thread, gated (main writes under the gate), audio thread. `Owned::with` lends a field to a closure that must not call into the plugin (its callbacks are the one way the same thread reaches the field again). Plugin calls go through `Plugin::call` (one unsafe place) and extension tables through `Ext<T>`.

## Watch out
Places outside the groups are kept unsafe and say so (`Owned::racy`, `as_ptr` writes); three are real data races in both hosts, left as they are because behavior must match Host.cpp:
- `wake_mirror`: Worker.cpp calls mirrorWake from its control thread after publishing the node to the engine, while the audio thread and the plugin's request_process read it (a plain pointer in Host.cpp too).
- `restful()` reads `carry_count` with no gate while loadState / a save's flush write it under the gate (Host::restful the same).
- `main()` writes `message` from a wrong thread (only when a caller breaks clap_host.h's thread rule).
`gui_floating` is read off the main thread in gui_request_resize but is sound: written only before `gui_created` is set.

## Ruled out
- Deref plus DerefMut to the whole Host (the obvious "one audited place"): unsound with the two threads.
- Allowing `unsafe_op_in_unsafe_fn` to drop lines (reverted earlier by the coordinator).
