---
title: "kick-clap-host: an orphaned helper stays a zombie under a container's PID 1, and a back-to-back audio loop starves the C++ host's gate"
author: Jacob
date: 2026-11-17
branch: flaky-two-b
cost: { minutes: 45, tokens: 2910051 }
id: l-987d3e08
scope: repo
status: active
src: agent
touches:
  - core/clap-host/tests/HostTests.cpp
---

## Decided
- The helper does end itself (stdin pipe EOF, Discovery.cpp watchParentPipe); the test's kill(pid, 0) still saw it because, orphaned, it went to PID 1, and a container without an init never reaps: a zombie answers kill(pid, 0). On Linux the test now makes itself the child subreaper and waitpid()s the helper. Old test: 2 of 2 failed without --init; new: 0 of 20.
- The TSAN timeout (29.65 s pass, 30.04 s timeout on GitHub) was the cross-thread wake/state loop: its audio thread rendered back to back holding the C++ host's gate (test_and_set) almost always, and loadState spins with yield() for it; on two cores that loop took 75 s under TSAN. A 50 us pause per block (as a real audio thread idles between periods): 2 s. Measure with taskset -c 0,1 in the TSAN container; on all cores it hides (8 s).
