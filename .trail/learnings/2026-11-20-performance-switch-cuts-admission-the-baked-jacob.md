---
title: "Performance-switch cuts: admission, the baked fader route, and the ring-out fade"
author: Jacob
date: 2026-11-20
branch: ringout-2
cost: { minutes: 7, tokens: 92450 }
id: l-6bc1887a
scope: repo
status: active
src: agent
touches:
  - core/control/src/budget.cpp
  - core/control/src/parts.cpp
  - core/control/src/performance.cpp
  - core/engine/rust/src/budget.rs
---

The owner's held notes cut at Performance changes (FANTOM-07, 2026-11-19) had two causes, both on this branch: admission held both Performances to ONE block's budget (budget.cpp estimateOf, now the per-core model with the outgoing one priced by its sounding voices, and a 150 ms fade on slot Session::retiringSlot past the CPU), and a level-0 send to an empty bus that plays where the part does, which merges with the part's fader route, mixes live slots, and bakes it (compileWires; master.set then only stages, and the change takes the tail to the staged master).

Ruled out: evidence (2) (Sampler -> another instrument plugin, A gone at +0.01 s) as a separate fault. control-performance-switch plays a Sampler through a Delay, pedaled chord, to a Basic Synth Performance: A rings as with no change, takes no new notes, goes after the pedal, with and without the empty-bus send. Reverting the level-0-send fix does not fail that case; it fails only the cases that lower the master before the switch (the unit's script did), so on the unit the drop came from the staged master. What took it to digital zero there is still not reproduced.

Watch out: raising a send on such an empty bus brings the merged, baked route back (fader heard at graph.commit only). The real fix is a route per set of live gains, not more special cases. Session::fade must see its 0 played before the nodes go: a late control-thread tick removed them at full level. Only one fade at a time: a second change during a fade rings whole. Test the fade with service() every 8 blocks (Audio::service), as the device ticks at 20 ms.
