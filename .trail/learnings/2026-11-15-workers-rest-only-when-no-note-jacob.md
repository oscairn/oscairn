---
title: "Workers rest only when no note can reach them in the plan playing (OSCAIRN_WORKER_REST=disabled), per the owner"
author: Jacob
date: 2026-11-15
branch: worker-rest-policy
cost: { minutes: 90, tokens: 105372 }
id: l-03ed38ab
scope: repo
status: active
src: agent
touches:
  - core/engine/src/graph.cpp
  - core/plugin-host/src/Worker.cpp
  - core/engine/tests/worker_sleep.cpp
  - core/engine/tests/worker_wake.cpp
---

## Decided
- The owner set the rest policy (2026-11-15): "can we avoid resting anything thats in the current performance", and a plugin may rest only if disabled and able to come back at once (a scene enabling it). Notes never glitching comes before idle CPU.
- OSCAIRN_WORKER_REST is three-valued (realtime.h workerRest): unset/disabled (default), all (any idle worker after the 192-block hold), 0 (none). The policy sits in graph.cpp's rest call (Plan::Step::playable), not in the run cores, so engine-run-diff is untouched.
- Playable, computed when a plan is built: a note connection not draining from the input or a playable step, audio from the input or a playable step, lanes in, or nothing coming in at all. Draining is what a part switched off (Session's disabled_, a scene's off parts) or a wire turned off compiles to (parts.cpp: live only for an enabled wire between parts not off). Mute is a live gain on the fader: notes still reach the instrument, so a muted part stays playable.
- Re-enabling is a new plan adopted at the block's start, before that block's notes are routed: the step is playable and called on that block. worker-sleep-disabled plays a note in the very block its connection comes back on: called, sounding, no block lost.

## Watch out
- In a Performance where every part can be played, the default saves nothing at rest (worker-bench, ST Final's shape on the Mac: callback p50 64 us bucket and the audio thread 1.5% of a core, the same as 0; `all` gives 16 us and 0.5%).
- An enabled part's first block after its rest is still cold (worker-wake with `off`: waking chain 113-130 us against 86-94 with 0); if the note comes in the enabling block it pays that.
- Kept-warm instances (Session's warm_) and a setlist's prepared next Performance are out of the graph: never called, nothing to rest. A retiring Performance's nodes stay in the plan with draining connections, so they rest once their tails sleep.
