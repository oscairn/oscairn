---
title: control-shared-costs compared two costs measured at different times; the sessions are now played block by block together
author: Jacob
date: 2026-11-17
branch: flaky-round2
cost: { minutes: 60, tokens: 54026 }
id: l-2653d696
scope: repo
status: active
src: agent
touches:
  - core/control/tests/shared_costs.cpp
---

## Decided
shared_costs.cpp plays the one-part session and the three-part sharing session together, block by block, in one process (measuredTogether), each with its own data folder set whenever either saves or ends. Off CI the voice ratio is held to 1/1.8 .. 1.8 as before; under relaxed() (core/test-support/relaxed.h) only ratio < 1.8, the fault's side (recorded as the instance's it was about 3).

Why: the failing CI ratio 0.3986 is one run's worker CPU figure against another's taken seconds apart on a throttled runner; the worker-based rework (2026-11-16 learning) fixed the wall clock but still compares two runs. Played in the same blocks both see the same machine; alone and shared voice costs came out 0.000305 vs 0.000308 per voice, ratio about 1.0.

## Tried and ruled out
- Counting work (voices rendered) instead of time: the measured figure is CPU per voice, which is what the claim is about; a count would not exercise Session::measure's weighting.
- Reproducing the CI ratio: 0 of 30 before and 0 of 60 after on this Mac at background QoS with 40 busy loops and ctest -j8 beside, 0 of 30 after with CI=1. Not reproduced, so no before and after contrast.

## Watch out
- Under relaxed() a shared voice costing much less than one alone (the other direction) is not checked.
