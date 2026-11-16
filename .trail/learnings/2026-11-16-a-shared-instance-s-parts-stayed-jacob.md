---
title: "A shared instance's parts stayed unmeasured; measuring's room ask each tick"
author: Jacob
date: 2026-11-16
branch: merge-candidate
cost: { minutes: 566, tokens: 725860 }
id: l-974e596e
scope: repo
status: active
src: agent
touches:
  - "core/control/src/budget.cpp#Session::measure"
  - "core/control/src/budget.cpp#Session::estimate"
  - core/control/tests/multi_sampler_load.cpp
---

decided: Session::measure records a shared instance's memory with the sounds it held (Measurement::sounds, "sounds" in measurements.json), keeping the fewest-sounds figure; the estimate of a saved Performance counts that instance on its first node and nothing for further parts up to that many sounds, then perContext. Before, memory was read only when one sound played (sounds == 1), so a sampler whose parts always loaded together in one instance never got a memory figure, perContext (which needs the one-sound figure) was never learned, and its parts were charged at guesses (unit: 53.6 MB guessed vs 42.9 MB held). perContext is at least 1 byte once an instance with more sounds is read: when contexts add nothing, readings' noise made bytes <= the one-sound figure, perContext stayed 0 and every further part was priced at a whole instance (control-multi-sampler-load failed 'count less than five instances' about 1 run in 3, unloaded). The room/sounds ask now happens once per memory reading (every 200 ticks) by the lead node: the per-tick per-part ask cost 1,000 worker requests a second for 5 parts and undid the shared ask of the engine-main work. ruled out: reading the engine's cached contexts listing for the room: it can lag a just-prepared sound, which makes room too high and the estimate low. watch out: service_cost only shows measure's cost with BUDGET=1 (an empty BUDGET= still counts as set: getenv returns ""); measure() returns at once without a budget.
