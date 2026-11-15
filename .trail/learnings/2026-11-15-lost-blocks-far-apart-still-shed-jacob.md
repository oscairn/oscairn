---
title: "Lost blocks far apart still shed voices: a loss within two seconds of the lane's last counts, down to each part's last voice"
author: Jacob
date: 2026-11-15
branch: guard-pedal
cost: { minutes: 1, tokens: 1858 }
id: l-bcb401a9
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/budget.rs
  - core/engine/tests/core_recovery.cpp
---

## Decided
12b also fires on a lost block whose lane lost another within 750 blocks (2 s), not only on two in its last eight. The loss and deadline rules never take a part below its last voice (a voice-budget limit of 1, else one note); a loss with every part on the lane at that floor is counted as `unfit` (engine.timing guard), meaning the chain after it has no room. A failed 12k probe is trouble with the cap at or past the ceiling.

## Tried and ruled out
On the unit (Sick tines pedalled), losses came about 1 per 180 blocks with the callback 100-250 us inside its deadline: never two in 8 (12b) or two in 4 slack bits (12d), so after the first burst nothing was shed. A test with slack NaN hides this: 12d's bits shift only on losses then, so far-apart losses look adjacent. Give the core a finite slack in such tests. Detecting a failed probe by "healthy for a probe's wait" missed probes whose losses are spaced, since the first loss resets the count.

In the code: core/engine/rust/src/budget.rs lost, LOSS_MEMORY_BLOCKS, choose (owned: floor), Entry::trouble; core/engine/tests/core_recovery.cpp sparse and unfit.
