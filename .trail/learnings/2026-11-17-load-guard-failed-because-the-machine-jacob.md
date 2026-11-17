---
title: "load-guard failed because the machine's own lost blocks reached the guard's wall-time rule"
author: Jacob
date: 2026-11-17
branch: flaky-three
cost: { minutes: 120, tokens: 2005979 }
id: l-b8b71d7c
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/load_guard.cpp
  - core/engine/rust/src/budget.rs
  - core/engine/rust/src/notes.rs
---

## Decided
load-guard's spike and device claims are checked only when the guard saw no more lost blocks than the test held up (Seen.machine, from noteCost lostBlocks and stolenForDeadline at the first give-up; the cheap lane's own lostBlocks in the device part). With more, the run says so and does not check that claim.

Why: the guard's rule is wall time by design (budget.rs lost: two lost blocks in eight or a loss within 750 blocks of another; notes.rs typical: median of five). A runner that stalls the worker or the engine thread adds lost blocks, so a note given up at block 201 (the sixth note, after two spiked blocks) is the rule working. Seen under taskpolicy -b with 40 busy loops and ctest -j8 beside: lostBlocks 25 or 3 where 1 was held up. Those runs also failed the one-block claim. On the GitHub runner the relaxed() five attempts still ended at block 201 (the log shows the last one; why each failed is not verified).

## Tried and ruled out
- Loading the Mac alone (ctest -j3 beside, 60 runs; 40 busy loops, 30 runs): 0 failures. The worker and test threads run real-time, so ordinary load does not reach them; background QoS plus heavy load did.
- A model clock in the fixture: the guard reads real worker time, so a model would test nothing of the rule.

## Watch out
- A claim that holds only when no block is lost beyond the injected ones cannot be proved on a runner that loses blocks; it is reported as not checked there.
