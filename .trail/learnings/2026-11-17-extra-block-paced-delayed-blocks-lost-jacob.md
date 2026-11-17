---
title: "extra-block-paced: delayed blocks lost are counted where the stall is, not across the run"
author: Jacob
date: 2026-11-17
branch: flaky-round2
cost: { minutes: 30, tokens: 77802 }
id: l-cb8ffe38
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/extra_block_paced.cpp
---

## Decided
The stall check reads the delayed workers' silentBlocks at call 400 and 10 calls later (stallCall, stallWindow; block i of the run is call i + 101) and asserts at most two lost there; the whole-run count is printed beside it.

Why: the old check counted every silent block of the delayed worker across 1500 blocks, so one lost to the machine anywhere made 'delayed blocks lost: 3' for a stall that cost two. The stall (KICK_FIXTURE_STALL 400:3700:0:1) is at a known call, and what it costs falls inside a few calls after it.

## Tried and ruled out
- Before and after contrast: the old binary was not kept, so no before rate; the single failure seen was on the coordinator's merge run. After: 0 of 15 at background QoS with 40 busy loops and ctest -j8 beside, 0 of 15 with CI=1.
- Best of three by delayed silent: a retry that hides the race.

## Watch out
- A block the machine loses inside the 10-call window still counts; the window is 10 of 1500 calls.
