---
title: "plugin-hosts identical compared a late block as if it were the hosts' sound"
author: Jacob
date: 2026-11-16
branch: tsan-three
cost: { minutes: 559, tokens: 941306 }
id: l-d5120563
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/plugin_hosts.cpp
  - core/plugin-host/src/WorkerHost.cpp
---

## Decided
identical() builds its two hosts with OSCAIRN_WORKER_BUDGET_US=500000 set, in hosts of their own (memory limit 5 GiB, which no other case uses), and plays with playUnbounded. The comparison with the engine then waits for a host's block however long a loaded machine holds it up.

Why: at a device's pace a block past the worker's allowance (half a period, 1.33 ms) is silenced, which is wanted, so a loaded sanitizer run differs from the engine at some block in its first 50 (hosts reported late 1, silent 1, misses 1). With 2 CPUs and 12 busy loops, ThreadSanitizer in Docker: 20 of 20 runs failed; 0 of 30 after. The retry gated on the late counter stays removed.

## Tried and ruled out
- Setting the variable before the Players only: a pooled host reads it when it starts (WorkerHost.cpp setRate) and the earlier cases' hosts, same memory limit, are reused with 1.33 ms. Hence the own memory limit.
- Offline mode: it gives every node a worker of its own, so no host is compared.

## Watch out
- A new case reusing the 5 GiB limit would inherit the 500 ms allowance in its hosts.

In the code: core/engine/tests/plugin_hosts.cpp identical(); core/plugin-host/src/WorkerHost.cpp setRate and the wait for a block.
