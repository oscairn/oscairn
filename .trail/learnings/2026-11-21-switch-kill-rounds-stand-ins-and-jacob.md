---
title: "Switch kill rounds: stand-ins and refusals from a just-killed plugin host; a stopped worker bounded during an open"
author: Jacob
date: 2026-11-21
branch: ringout-4
cost: { minutes: 2804, tokens: 10792046 }
id: l-d0b0f54a
scope: repo
status: active
src: agent
touches:
  - core/plugin-host/src/WorkerHost.cpp
  - core/control/src/performance.cpp
  - core/control/tests/performance_switch.cpp
  - core/control/tests/performance_switch_stress.cpp
  - core/control/CMakeLists.txt
---

## Decided
- The kill rounds' "no parameter 1 on node 10" / "no parameter 7 on node 11" (control-performance-switch-stress-hosts) was a stand-in: a fresh load picked into a plugin host killed a moment before, not yet seen by its supervisor; the load's connection closed and resolvePlugin (performance.cpp) took the runtime_error for a plugin that cannot load, so the delay or synth opened as Unavailable with no parameters and g.parameter threw. loadPluginInWorker now loads once more in a worker of its own when a pooled host's connection broke under the load (a plugin that really crashes on load takes that down too and stands in as before; a hang, lost() without `broken`, is not retried).
- The same rounds' "cannot restore X: Plugin worker connection lost": the supervisor counted the host's death before readTaking's pass took its failure count, so the broken restore raised no count and the open was refused. Process::exchange counts a broken connection in workerFailures at once, and Process::failure() says "Plugin worker connection lost" from then until the restart, so the next pass skips the instance (fits) and the notice names its nodes. workerFailures is read only by readTaking.
- Base rates (ringout-3 on ced2ca13): hosts 12 of 300 on the Mac (6 at once), 6 of 100 in a 2-CPU container; a worker per plugin 0 of 300 and 0 of 100. After: 0 of 300 hosts, 0 of 300 workers on the Mac. Likely why the older base showed none: before 3297506b a killed worker's socket was often inherited by another worker and stayed open, so requests timed out and lost() counted the failure synchronously.
- ReplyBound (plugin_host.h), held by readTaking: a request (not a load) fails the worker after 500 ms without progress (no CPU time; on Linux the main thread not R or D). Round 0 of the stopped-worker test runs at the default 30 s timeout now.
- The 240 s switch timeout was not reply waits: control-performance-switch-workers is ~40 rendering cases; 54 s on --cpus 2, 124 s only with the Mac also running 14 stress tests. Back to 120.
- control-performance-switch-workers' TSAN failures (2 of 5, "all kept warm while nothing is opened", "1 loaded"): the test's budgets left out m.reserved (workerCode, the worker binary's code) which evictOpportunistic counts. Both budgets now add it and sit half the instrument's memory from the edge.
- One route per set of live gains (9fd74e93) adds no counted work: engine-cost-budget(-core) unchanged (136/232; 10 handed, 15,360/19,380 bytes, 12.6/15.6 calls). The extra routes exist only for a part's nonzero send to an empty bus playing where the part does; about 7 ns per extra route per 64-frame block on this Mac (a throwaway Graph bench, 8 such parts: 199 ns/block baked, 16 routes, against 310 ns live, 32 routes). The benches (rest, session) build graphs without live slots, so they cannot show it.
## Watch out
- On macOS awaitReply's progress is CPU time alone (Linux also counts the main thread in R or D): a worker blocked on I/O with no CPU for 500 ms during an open is failed and loaded afresh.
- A pooled host found dead at a load marks the plugin crashedIn (own worker from then on), as before, even when the host was killed from outside.
