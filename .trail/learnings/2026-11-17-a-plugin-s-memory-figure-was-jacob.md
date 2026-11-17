---
title: "A plugin's memory figure was the last instance read, so estimates came in under what was held"
author: Jacob
date: 2026-11-17
branch: ci-red
cost: { minutes: 1066, tokens: 1674888 }
id: l-2e33e1d1
scope: repo
status: active
src: agent
touches:
  - "core/control/src/budget.cpp#Session::measure"
  - core/control/tests/multi_sampler_load.cpp
---

decided: Session::measure keeps the plugins whose memory a memory tick (every 200th measure) has read, and raises a plugin's measured_ figure to each further instance's reading in that tick (sounds 1, or a shared instance with as many sounds as the figure's). Every instance of a plugin is priced at that one figure, so last-read-wins under-counted: control-multi-sampler-load apart priced five sampler copies at the organ's 17.7 MB while three held 18.4-18.5, and twelve effects at the last one's 3.64 MB while some held 3.72 (130 MiB estimated against 134 held, on a quiet Mac). The test now plays, then rests 200 service ticks (no process) before reading the workers, so the estimate's figure and the held reading are of the same quiescent state (the shared sampler's figure then equals its held bytes exactly), and its 2 MiB slack is gone. ruled out: the reading-time skew alone (the earlier theory): after quiescing, the gap stayed until the max fix. watch out: a figure still falls when every instance shrinks; the per-tick max is only across the instances read in that tick.
