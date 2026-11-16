---
title: "An open Performance's further sampler parts counted at the effect-context rule"
author: Jacob
date: 2026-11-16
branch: unit-findings-2
cost: { minutes: 720, tokens: 1012454 }
id: l-156d539a
scope: repo
status: active
src: agent
touches:
  - "core/control/src/budget.cpp#Session::nodeCost"
  - "core/control/src/budget.cpp#Session::furtherPart"
  - core/control/tests/multi_sampler_load.cpp
---

decided: the unit's 9,912,720 bytes a further Sampler part (TSO Carol Bells) was not a guess from measurements.json: it was nodeCost's effect-context branch, which SoundContext nodes take because they share the engine's effectGroup (Graph::effectSource gives the lead), charging context_bytes + asset_bytes, the Sampler 0.9.2's 331,656 + 9,581,064 (its declared total 13,957,800 less shared 4,045,080), with the instance's assets once per part on top of the instance's measured 44,134,400 (record sounds 5, perContext 0). The preflight estimate(dir) had the rule; budget.estimate without a folder, Session::add's check, committedMemory beside a load and memoryRoom use nodeCost and did not. Both now go through Session::furtherPart, a part ranked by its channel past the lead. control-multi-sampler-load only estimated Performances not open, so it never saw it; it now opens one with the unit's record and fails before the fix (69 MiB against 65 held, 0 parts at nothing). ruled out: perContext 0 read as unknown (the coordinator's guess): the open path never read the measurement for a further part. watch out: the preflight merges every record of a build (no declaration to pick one), most memory, fewest sounds, largest perContext: on the unit's records a further part there counts 25,141,248 bytes, one noisy record's perContext. Not fixed.
