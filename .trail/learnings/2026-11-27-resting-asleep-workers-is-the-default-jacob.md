---
title: "Resting asleep workers is the default: the owner replaced the 2026-11-15 rule on 2026-11-27 (OSCAIRN_WORKER_REST unset is all)"
author: Jacob
date: 2026-11-27
branch: defaults-1127
cost: { minutes: 60, tokens: 300000 }
id: l-68e0a31f
scope: repo
status: active
replaces: l-03ed38ab
src: agent
touches:
  - core/plugin-host/src/Worker.cpp
  - core/plugin-host/include/oscairn/realtime.h
  - core/engine/src/graph.cpp
  - core/engine/tests/worker_rest_diff.cpp
---

## Decided
- The owner changed the rule on 2026-11-27 (issue 7), after the FANTOM measurement on main 272188a0, ST Final, 13 workers: with all the engine thread took about 20% less at rest (about 550-565 us a 128-frame block against 675-710), 40 notes after more than 1 s of silence (20 alone, 20 with ten held keys on another part) lost no block (late, silent, deadline-missed, underrun all 0, missPanics 0). A playable part may now go cold: workerRest() in Worker.cpp reads unset, "all" and anything unknown as all; "disabled" keeps the 2026-11-15 behavior (graph.cpp Plan::Step::playable decides); "0" calls every worker every block. worker-rest-diff compares unset, all and 0 bit for bit.
- The Sampler does not rest on the FANTOM while it is in the extra block: WorkerHost::rest returns false for OSCAIRN_WORKER_EXTRA_BLOCK nodes and fantom-07.json lists org.oscairn.sampler there (issue 318 removed VA). Bass, Pad, Pad 2, Sweep and K2 were called every block with rested 0 under all as well. What all rests on ST Final is the Arp chain and the Variation chorus.
## Watch out
- The earlier learning stays true for "disabled" and for the cost of a cold first block (+12-20 us on the Mac, worker-wake). Not measured: the Sampler waking from rest, the arp chain onset (the arpeggiator grid hides it), listening.
