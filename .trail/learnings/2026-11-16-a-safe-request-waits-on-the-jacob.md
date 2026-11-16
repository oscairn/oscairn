---
title: "A safe request waits on the load gate by polling, not a timed wait"
author: Jacob
date: 2026-11-16
branch: busy-latency
cost: { minutes: 231, tokens: 463022 }
id: l-0664db3c
scope: repo
status: active
src: agent
touches:
  - core/clap-host/rust/src/host.rs
  - core/clap-host/rust/src/more.rs
  - core/clap-host/src/Host.cpp
  - core/clap-host/tests/HostTests.cpp
  - core/clap-host/rust/src/lib.rs
  - core/control/tests/parts.cpp
  - core/engine/tests/clap_host_rust.cpp
  - core/engine/tests/worker_recovery.cpp
  - core/plugin-host/src/WorkerHost.cpp
  - core/engine/tests/worker_wake.cpp
---

## Decided
- The helper waits for the load gate with try_lock polled on the steady clock for at most 2 ms (gateWait), not a timed wait. CI's control-not-blocked-taken failed: every refusal came back at 100.2 ms against the 100 ms bound.
- Measured on this Mac: a 10 ms try_lock_until (system clock) or try_lock_for takes 15 ms; the test's refusals took 15 ms. Both clocks alike, so the TSAN workaround (3f52e677) was not the cause. The CI runner's timer lateness is inferred, not seen.

## Tried and ruled out
- Reproducing 100 ms locally with 24 busy processes: still 15 ms.

## Watch out
- A refusal costs gateWait plus the helper's pass and the client's scheduling; keep gateWait small against the 100 ms bound.
- try_lock is also what GCC 13's ThreadSanitizer sees; clang TSAN on macOS passes, GCC on Linux not run.

In the code: core/control/src/local_socket.cpp refuse(), gateWait; docs/control-protocol.md Long requests.
