---
title: "A panic must not drop the parameter changes queued with it: that was the flaky worker-restart test, not lost ports"
author: Jacob
date: 2026-11-15
branch: ci-green-fix
cost: { minutes: 29, tokens: 187728 }
id: l-d2f80e1e
scope: repo
status: active
src: agent
touches:
  - core/control/tests/parts.cpp
  - core/clap-host/rust/src/host.rs
  - core/clap-host/src/Host.cpp
  - core/clap-host/tests/HostTests.cpp
  - docs/plugin-porting.md
  - core/engine/rust/src/run.rs
---

## Decided
When a CLAP host resets a plugin for a panic, the parameter values, modulation and gestures gathered for that block are still sent, after the reset. Only notes and controllers are dropped. Both hosts (C++ and Rust) do this.

Why: the caller already holds a queued parameter change as set (the host stores the value when it queues the event), so one dropped by the reset stays lost for good. The engine panics a worker node whose block was neither processed nor kept. After a worker restart under CPU load the new worker's first block is late, the engine panics it, and the restore's SetParameter, still in the queue, was thrown away with the panic. The same loss would hit any knob change that meets a panic or a missed worker block.

## Tried and ruled out
- "The ports are not told again after the restart" (the test's message, and the earlier agent's suspicion): the level 5.5 is insertion B's gain back at its default 0.5 instead of 0.25 (4 + 0.5 + 2*0.5). Ports not told would give 7.
- A failed restart attempt capturing the new worker's defaults: debug prints showed no failed attempts; every failing run had `panic()` reaching the restarted node with its worker up.
- Lengthening the test's wait (5 s to 30 s): the value never comes back, and 3000 passes under load ran past the test's 120 s limit, which is the likely cause of the macOS "Timeout" with no FAIL line (not confirmed on CI).

## Watch out
- Reproduce with load, not alone: the test passes in 0.2 s by itself; 12 at once with 48 busy loops failed about 1 run in 30 before the fix, 0 of 480 after.
- Not checked: whether the controller state replayed in a restarted worker's first block (bend, pedals, per-channel values) is lost the same way when that block is panicked; those are not parameter events and are still dropped.
- The Rust host's change is covered only by the comparison test (clap-host-rust), which panics once; whether a parameter event meets that panic there is not checked.

In the code:
- core/clap-host/src/Host.cpp, Host::process, the `s.panic.exchange(false)` branch; core/clap-host/rust/src/host.rs, the `panic.swap` branch.
- The panic's sender: core/engine/rust/src/run.rs (`lost = !processed && !p.kept(s)`), reaching WorkerHost::panic.
- Tests: core/clap-host/tests/HostTests.cpp ("a parameter queued before a panic is kept"); core/control/tests/parts.cpp portSplitsInWorker (`parts-test ... --ports-workers`, with OSCAIRN_PLUGIN_WORKER and OSCAIRN_WORKER_BUDGET_US=50000).
