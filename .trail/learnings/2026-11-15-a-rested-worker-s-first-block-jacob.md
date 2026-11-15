---
title: "A rested worker's first block costs a cold wake-up; rest only after a 0.5 s hold, and the beat no longer wakes a plugin asleep"
author: Jacob
date: 2026-11-15
branch: worker-wake
cost: { minutes: 44, tokens: 57263 }
id: l-edec2f7d
scope: repo
status: active
src: agent
touches:
  - core/plugin-host/src/WorkerHost.cpp
  - core/clap-host/src/Host.cpp
  - core/engine/tests/worker_sleep.cpp
---

## Decided
- A rested worker's first called block costs more than a steady one: worker-wake (instrument + 5 sleeping effects, each in a worker, 128 frames, Apple silicon) measured each worker's wake (hand-over until its audio thread ran) about 8 us against 4-5 steady and its round trip about 20 us against 11-13, so the chain's waking block took 145 us against 80 with every worker called (OSCAIRN_WORKER_REST=0). The plugins add nothing of their own: the effects' owner measured their first block after sleep as any cold block, and with rest off the plugin's DSP was skipped while asleep anyway, so the extra is the worker's own path (its thread parked up to 1 s, its code, stack and shared block cold), paid once per worker, one after another in a serial chain.
- The fix kept: WorkerHost::rest rests a plugin only after restHold (192 blocks, 0.5 s at 128 frames) blocks in a row it could have rested; until then it is called as before. A chain fed a moment ago (an arpeggio's gaps, a phrase's) stays warm and only real silence costs a wake. Idle saving kept: idle block 3.5-7.6 us against 61-120 us with rest off (5 effects, machine load varied).
- OSCAIRN_WORKER_REST=0 (read once, WorkerHost.cpp's workerRest) calls every worker every block; passed by oscairn-fantom-start from etc/default/oscairn-engine.
- The beat and harmony no longer wake a plugin asleep (kick::clap::Host::process counts them apart; restful() no longer excludes plugins that asked for them): the arpeggiator returns CLAP_PROCESS_SLEEP when idle (its plugin.cpp, confirmed by its owner), and the host's beat every block was what kept it called (66 us a block on the unit). A harmony held back on a skipped block is sent again with the waking one (haveHarmony cleared). The beat's tempo only changes with the transport, which still wakes.
- engine.timing gives each worker's `asleep` (its plugin's last status): asleep yet called on most blocks means the host keeps it awake.

## Tried and ruled out
- Pre-waking rested workers' audio threads at the start of a block with input events (ring the doorbell, no request): interleaved A/B 3x30 cycles, 5 effects: waking chain 157 us with it, 133 without, 97 rest off. The futex wakes on the engine thread cost more than the warmer threads saved, and the woken thread parks again at once. Spinning them instead would starve the working worker on the FANTOM's 4 cores (SCHED_FIFO, more workers than cores).

## Watch out
- Absolute numbers are from the Mac; the A53 is several times slower per wake. The unit must confirm with OSCAIRN_WORKER_REST=0 against 1 on the same takes.
- The first note after more than 0.5 s of silence still wakes the whole chain cold, one worker after another.
- Sick tines' losses came with keys held: its chain is fed every block, so nothing there rests; called/rested per worker on that run says whether resting is involved at all.
