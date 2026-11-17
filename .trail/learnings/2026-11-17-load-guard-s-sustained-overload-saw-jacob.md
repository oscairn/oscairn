---
title: "load-guard's sustained overload saw no note given up under ThreadSanitizer: the losses' rule acted first"
author: Jacob
date: 2026-11-17
branch: flaky-two-b
cost: { minutes: 30, tokens: 3217325 }
id: l-5069e003
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/load_guard.cpp
---

## Decided
Under ThreadSanitizer the overloaded worker's block (5 notes + 0.4 ms) passes its 1,000 us allowance, the block is lost, and lost() fades the notes (stolenForLoss) at block 201-202, before measured() would fade for the lane's limit. The test counted only stolen + hardChokes, so all five attempts showed "none" (2 of 10 runs in a Linux TSAN container; 7 of 10 runs gave notes up only for losses). The sustained claim now counts every give-up kind (Seen.gone); the one- and two-block spike claims still count the lane's fades and chokes alone, since a held-up block there is expected to be lost.
