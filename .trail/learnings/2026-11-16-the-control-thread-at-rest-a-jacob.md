---
title: "The control thread at rest: a request to every worker each 20 ms, five times over for a shared instrument"
author: Jacob
date: 2026-11-16
branch: unit-findings
cost: { minutes: 120, tokens: 994930 }
id: l-2537ef6c
scope: repo
status: active
src: agent
touches:
  - core/plugin-host/src/WorkerHost.cpp
  - core/engine/src/sound_context.cpp
  - core/control/src/session.cpp
---

## Decided
- Sick tines at rest held the engine's main thread at 73 to 89 percent of a core (night J3). What it does each 5 ms tick (Session::service): Graph::service asks every worker to service its plugin (a request, an answer, two switches), at most every 20 ms on a device; on a Mac with 25 plugins in workers a tick took 241 us. Each of five SoundContexts on one instance asked the same worker again (a service and a sound listing each): now one pass asks and the contexts read the answer (Engine::serviced, read). A worker that answered nothing 25 times is asked every 100 ms until it says something or is asked anything that changes it (WorkerHost::quiet_). Result (service_cost, 8 s): 217 us and 1,431 requests a second with the shared ask alone, 142 us and 403 with the quiet back-off too (shared-engine, worker-service tests).
- Session::model built a full graph description each tick for its mappings alone: Graph::mappings (142 to 126 us).

## Tried and ruled out
- The refused Output bus value (d5b0154b on main): SoundContext::setParameter refuses with no request to the worker, a stuck value cost 10 us a tick (STUCK=1, service_cost). It blocks saves, it does not spin.

## Watch out
- Left: Session::record builds a Model each tick (about a third of what is left), 22 supervisor threads wake every 10 ms (not the main thread). Unmeasured on the unit.

In the code: WorkerHost::service, SoundContext::service; tool core/control/tests/service_cost.cpp.
