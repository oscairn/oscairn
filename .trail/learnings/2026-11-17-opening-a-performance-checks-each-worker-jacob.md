---
title: "Opening a Performance checks each worker for an exit first, so a worker that just died is never taken over"
author: Jacob
date: 2026-11-17
branch: unit-findings-3
cost: { minutes: 1361, tokens: 2236223 }
id: l-b42de87e
scope: repo
status: active
src: agent
touches:
  - core/control/include/oscairn/busiest_load.h
  - core/control/include/oscairn/control.h
  - core/control/include/oscairn/session.h
  - core/control/src/budget.cpp
  - core/control/src/performance.cpp
  - core/control/tests/admission.cpp
  - docs/control-protocol.md
  - docs/rfcs/0025-resource-budgets.md
  - core/control/tests/value_reports.cpp
  - core/engine/include/oscairn/sound_context.h
---

## Decided
- Session::readTaking (core/control/src/performance.cpp) calls checkWorkers() (WorkerHost.cpp, Process::exitedNow) before it collects instances to take over. Each worker process gets a non-blocking waitpid. One that exited is failed then, with the same text and blame its supervisor's tick would give, and the supervisor is woken to restart it. After that, every reuse path's existing failure().empty() filter skips it, the way any known-failed instance is skipped: its nodes load afresh with the document's state, and the opened Performance's notices name the last Performance's plugins whose worker had stopped.
- exitedNow only try_locks the supervisor's mutex and the request lock. A supervisor busy with a tick or a restart already has the worker, so the control thread never waits.
## Tried and ruled out
- Waiting for the supervisor to restart the worker during the load: an unbounded wait on the control thread, against the rule.
- Catching "connection lost" at each restore path (restoreTaken, claim, joinEngine, effect contexts): too many paths. The window is the supervisor's 10 ms tick, so seeing the death first closes it. Only restoreTaken's state-equality check, which was outside any try, now falls back to a fresh load.
## Watch out
- A worker dying during the load, after the check, can still fail the open in a path without a fallback.
- The test peeks with waitid(WNOWAIT): reaping the child itself would hide the exit from the supervisor.
