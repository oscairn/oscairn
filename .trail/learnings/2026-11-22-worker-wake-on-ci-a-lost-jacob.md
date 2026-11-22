---
title: "worker-wake on CI: a lost block begins a worker's hold again, and the test's bound on idle calls had no room for it"
author: Jacob
date: 2026-11-22
branch: worker-wake-idle
cost: { minutes: 150, tokens: 12532691 }
id: l-a9c5d567
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/worker_wake.cpp
  - core/plugin-host/src/WorkerHost.cpp
---

worker-wake's CI failures (macOS bd7392a2: 11652 calls, ubuntu b136f0dd: 12175, of 15600 = 260 rest blocks x 10 cycles x 6 workers, the registered configuration '10 260 128 5 off') are not workers failing to rest. The old bound was (hold 192 + 2) x 10 x 6 = 11640, itself 75 percent of 15600, and an unloaded run makes exactly 11520 calls (192 per worker per rest); CI was over by 12 and by 535. Cause: WorkerHost's rest() never rests a block whose worker answered the last one late (s.done != sequence_), and start() then sets restful_ = 0 because holding_ is false, so each lost block (late or silenced) begins that worker's 192-block hold again. On a runner with fewer cores than the test's 7 real-time threads the test already forgives lost blocks ('crowded'), but still held the calls to the no-loss figure. Measured in oscairn-ci-rust:24.04, --cpus 2 with 6 busy loops: 20 of 20 runs fail the old bound on b136f0dd, on ced2ca13 and on 30781e0c alike (calls 12043 to 13705), 0 of 20 on 2 CPUs without the loops, 0 of 12 on the Mac under 40 busy loops. Over 3600 worker-rests of the loaded tip runs: every rest in which the worker lost no block made 192 or 193 calls (962 of them); every rest over 194 (1121) had a lost block of that worker in it; none exceeded hold x (lost + 1) + 2; every worker rested again in later cycles. So nothing latches, and the idle-cost and ring-out series changed nothing here (same rest() and start() lines at 30781e0c). The test now checks each worker in each rest: calls <= hold x (blocks it lost in that rest + 1) + 2, keeps 'never every block of the run', and on failure prints calls/lost per worker per rest, which tells a hold begun again (lost > 0) from a worker that will not rest (lost 0, calls past the hold). Off CI a lost block still fails the run, so the allowance never applies there. Ruled out: widening the total bound (would hide a worker that never rests in some cycles); changing the product so a late idle block keeps the hold (the late block's reply is what says restful and asleep; a worker just late kept warm 0.5 s is the hold's own purpose). Not explained by this: the keyboard's 20-27 us/block difference between starts; engine.timing's per-worker called, rested, late and asleep would show a worker called while asleep there, but with OSCAIRN_WORKER_REST unset a worker a note can reach never rests by design, on every start alike.
