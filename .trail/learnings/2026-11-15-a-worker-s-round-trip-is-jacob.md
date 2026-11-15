---
title: "A worker's round trip is two scheduler wake-ups; fusing serial chains, then chain-aware hosts, removes the floor"
author: Jacob
date: 2026-11-15
branch: worker-floor
cost: { minutes: 70, tokens: 265779 }
id: l-49d33f06
scope: repo
status: active
src: agent
touches:
  - core/plugin-host/src/WorkerHost.cpp
  - core/plugin-host/src/Worker.cpp
  - core/engine/src/run_cpp.cpp
  - docs/rfcs/0045-audio-scheduling-on-small-devices.md
---

## Decided
- What a worker's ~45 us per block is, with its plugin asleep: two scheduler wake-ups, not the plugin and not copying. Real code (oscairn-worker-wake-test, OSCAIRN_WORKER_REST=0, arm64 Linux container on 4 CPUs, C++ core): round trip 36-48 us per worker, of which the worker's wake (hand-over until its audio thread ran) 17-18 us, its render of the asleep node 4-5 us (6-8 on the first block after a rest), the rest the engine's own wake-up from FUTEX_WAIT on done. Port copies (1 KB each way) are under a microsecond. The container's idle cost per worker (36 us) is close to the unit's 35-45.
- How they line up: within a wave every worker is handed its block and then collected in plan order; waves run one after another (run_cpp.cpp / run.rs). A 22-worker Performance like Sick tines (shared instrument, 5 parts x 3 inserts, 5 sends, master, as assumed) is 6 waves: 6 engine sleep/wake cycles on the critical path, 22 futex wakes and up to 22 engine switches serial on the engine thread, the renders overlapping on the other cores. That is engine CPU, which is why the unit shows a third of a core.
- The design recommended (not built): fuse serial chains in one worker process (a node whose only audio consumer is the next, unity 1:1 route, no live slot, no notes or lanes into the follower from its wave), the worker copying the head's output into the follower's input and rendering it in the same pass: a follower then costs its render (~5 us) instead of a round trip; then chain-aware placement in at most cores-1 hosts (a chain the unit of placement, the heavy instrument alone, which is what the 3-host test on the unit lacked: Analog and a Sampler landed in one host), where the existing spin rule (threads <= cores) applies.
- Modelled bench (.scratch/floor_bench.cpp on branch worker-floor, the protocol not the real code, quietest of noisy rounds, block p50 for the 22-worker shape): every worker called 476 us; chains fused, 12 processes 295 (0.62); 3 hosts fused 166 (0.35); 3 hosts + 300 us spin 55 (0.12); rest 32. First block after 1 s: after rest 549-837, called 500, fused 300, hosts 181-211, so fusion also shrinks the cold first block the owner worries about.

## Tried and ruled out
- Keep-warm by ringing asleep workers without waiting: 21 futex wakes cost the engine thread 94-136 us a block in the container, and the next real block was not warm (p50 0.8-2.1 ms against 0.5 called): 21 FIFO threads woken on 4 cores each block. Same verdict as the earlier pre-wake test on the Mac.
- Spin for 22 or 12 processes (more threads than cores, the existing rule forbids it), batched doorbells (no system call wakes many futex words; one shared word wakes every worker each wave), skipping silent port copies (under 1 us).

## Watch out
- Fusing a chain loses isolation inside it only: a crash restarts every plugin of that part's chain, which was silent anyway because its audio passes through the dead one; the restart replays each node (WorkerHost::restore, as plugin hosts already do).
- The worker must apply the engine's output guard between fused nodes, or a NaN from the head reaches the follower where unfused it would not; a fused follower's rest must follow its head.
- Container numbers are relative; the unit must confirm with oscairn-engine-timing per worker (wake, round trip) before and after each stage.
