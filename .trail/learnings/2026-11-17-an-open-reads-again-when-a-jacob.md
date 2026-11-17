---
title: "An open reads again when a worker failed during the read, so a worker still dying at the switch no longer refuses it"
author: Jacob
date: 2026-11-17
branch: unit-findings-4
cost: { minutes: 1537, tokens: 2618737 }
id: l-ba6eb230
scope: repo
status: active
replaces: l-b42de87e
src: agent
touches:
  - core/control/src/performance.cpp
  - core/plugin-host/src/WorkerHost.cpp
  - core/plugin-host/include/oscairn/plugin_host.h
  - core/control/tests/performance_switch_stress.cpp
---

## Decided
- Session::readTaking (core/control/src/performance.cpp) reads again when read() throws and workerFailures() (WorkerHost.cpp, counted in Process::fail and lost) rose during the pass. Before the next pass, checkWorkers() also fails a worker whose connection broke under a request (Process::exitedNow: `broken` set, not yet waitable, so SIGKILL and a blocking waitpid, which takes the moment it was dying anyway). A worker that gave no reply in its timeout is failed by lost(). The next pass skips both as any failed instance, and their nodes load afresh. At most one extra pass per instance offered.
- Why: on the unit a SIGKILL and the load sent in the same instant were refused 3 of 3. The killed worker had not exited when the open's check ran, so checkWorkers() alone could not close the window.
- The send in Process::exchange now sets `broken` too. A worker that has already gone fails the send, not the receive.
## Tried and ruled out
- Counting failure() over the offered instances to decide whether to read again: the supervisor restarts a worker failed by a lost reply within the read, which clears failure(), so the count did not rise and the open was refused anyway. The global failure counter rises at every failure.
- Naming in the notice only the nodes failed after the read: the existing kill-at-switch rounds lost their notice, because those workers were already restarted. The notice now names the nodes failed at the check plus those failed when a pass threw.
## Watch out
- Each pass that throws repeats the whole read, fresh loads included. A dying worker costs one more read. A hung one costs its reply timeout (30 s, OSCAIRN_WORKER_REPLY_SECONDS) per hung worker, unless the supervisor fails it first for missed blocks while audio plays.
- A fresh load picked into a plugin host that hangs waits a load's full 30 s. That is why the stress test's "no reply" round runs only with a worker per plugin.
