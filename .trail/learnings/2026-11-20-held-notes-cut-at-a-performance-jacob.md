---
title: "Held notes cut at a Performance change come from admission's seamless test, not the ring-out routes"
author: Jacob
date: 2026-11-20
branch: ringout-routes
cost: { minutes: 2079, tokens: 5638101 }
id: l-8927c529
scope: repo
status: active
src: agent
touches:
  - core/clap-host/tests/HostTests.cpp
  - core/engine/tests/load_guard.cpp
  - core/engine/tests/vectors/engine-core.vec
  - core/engine/tests/vectors/engine-run.vec
  - docs/developer-guide.md
  - core/engine/rust/src/notes.rs
  - core/engine/src/PreparedInstrument.cpp
  - core/plugin-host/src/WorkerHost.cpp
  - core/control/include/oscairn/control.h
  - core/control/src/json_rpc.cpp
---

## Decided
- A Performance change cuts held and pedaled notes only when admission calls it not seamless. The ring-out routing keeps them in every chain tried: insert, send and shared-instance chains; B using the same synth and effect, the same synth with another effect or none, or another synth with the same effect. Each one sounds the same as with no change.
- A cut that ends a note held, pedaled or heard in the last 100 ms now leaves a session.status notice: "the last Performance's notes could not ring out". It had only gone to stderr, so the player never learned why.
- The estimate is left as it was (the estimates rework belongs to another branch). It is very cautious: the open Performance and the incoming one are both priced at full cost, and the incoming one's beside-claimed instance has its memory counted twice. The CPU sum is checked against one block's budget, not budget x cores. The fix proposed is in RFC 0021, "Held notes across other chains".
## Tried and ruled out
- Blaming Graph::ringOut's all-or-nothing rule (a route into a node that isn't retiring makes no routes). A change never reaches it: freeInstances marks the held note's instance and every node it feeds as busy, so none of them is taken over, and an effect with contexts gets a new context while its old node rings out. If it were reached, the plugin's retiring sound would fall back to its main output and play through the new chain. It would not go silent.
## Watch out
- The Basic Synth's sustained sound decays on its own. "Rings on" has to be compared with the same notes played with no change (a second Session), not with the level before the change.
In the code: core/control/src/performance.cpp openPerformance (the cut notice), Session::freeInstances; core/control/src/budget.cpp estimateOf's e.seamless; core/control/tests/performance_switch.cpp "ring out, ..." cases.
