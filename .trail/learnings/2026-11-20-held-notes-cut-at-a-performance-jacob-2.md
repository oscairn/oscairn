---
title: "Held notes cut at a Performance change: the ring-out fit rule and a level-0 send to an empty bus, not the ring-out routing"
author: Jacob
date: 2026-11-20
branch: ringout-routes
cost: { minutes: 2135, tokens: 5706485 }
id: l-845f38fa
scope: repo
status: active
replaces: l-8927c529
src: agent
touches:
  - core/control/include/oscairn/control.h
  - core/control/include/oscairn/session.h
  - core/control/src/budget.cpp
  - core/control/src/json_rpc.cpp
  - core/control/src/parts.cpp
  - core/control/src/performance.cpp
  - core/control/src/remote_control.cpp
  - core/control/src/session.cpp
  - core/control/tests/parts.cpp
  - core/control/tests/performance_switch.cpp
---

## Decided
- The unit's cuts had two causes. First, the ring-out fit rule: the open Performance at full cost plus the incoming one had to fit one block's budget. They are now held to the per-core model an ordinary open uses (total within budget x cores, each chain within one block). The outgoing Performance is priced by what still sounds (Session::ringingCost). Past the CPU with the memory there, its tail fades out in 150 ms on its own live gain (Session::retiringSlot, Session::fade) instead of a cut. Past the memory it is still a cut.
- Second, a part's level-0 send to an empty bus the converter did not build. That bus plays where the part does, so the send's wire and the fader wire compiled to one route with different live slots. The route was baked, so master.set only staged an edit (12 dB high on the unit) and the change took the tail to the staged level. compileWires now leaves a level-0 send out and marks it a static send.
- Every cut or fade that ends a sounding note says so in a session.status notice.
## Tried and ruled out
- "A's instance is released when B does not claim it" (the Sampler to VA cuts): every A that cut that way had the empty-bus send. Instances not claimed are carried over and ring, and the "(iii)" cases in control-performance-switch show it.
- Graph::ringOut's all-or-nothing rule: a change never reaches it, because freeInstances keeps the chain busy. It is now partial anyway, and said in a notice.
## Watch out
- In the test, the level-0 send reproduced a tail drop of 11 dB, not the unit's drop to digital zero. What took it to zero there was not found. The fix makes the Performance compile as the copy without the send, which rang on the unit.
- The estimate's cores follow OSCAIRN_AUDIO_CORES. Tests that check the fit rule set it to 3 (or 1), or they depend on the machine's core count.
In the code: core/control/src/budget.cpp (estimateOf, ringingCost), core/control/src/parts.cpp (compileWires' unsentSend, Session::fade), core/control/src/performance.cpp (openPerformance, open), core/engine/src/graph.cpp (ringOut, fadeRoutes).
