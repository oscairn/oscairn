---
title: "Issue 7: the per-block exact skip already exists as rest; the default keeps playable workers called, and what an idle call costs"
author: Jacob
date: 2026-11-27
branch: worker-idle
cost: { minutes: 4159, tokens: 20894100 }
id: l-343d35c2
scope: repo
status: active
src: agent
touches:
  - core/plugin-host/src/WorkerHost.cpp
  - core/engine/rust/src/run.rs
  - core/engine/src/graph.cpp
  - core/clap-host/src/Host.cpp
  - core/clap-host/rust/src/host.rs
---

## Decided
- What issue 7 asks for (a worker whose plugin sleeps, with no input, events, wake or flush, not called that block, its note's block still called) is RFC 0045's rest as built: Run::rest (run.rs) decides per block from what the engine holds (no core-space event merged for the step, Audio::quiet on its inputs, no transport_wake) and WorkerHost::rest adds the worker's side (restful: plugin said CLAP_PROCESS_SLEEP, no wake/flush/panic/carry; the wake word mirrors request_process, host commands, selects, chokes; not busy, not a block late, no replay). engine.timing already has `called` and `rested` per worker. What keeps a playable worker called is only the policy: OSCAIRN_WORKER_REST unset (disabled) rests only steps no note can reach (graph.cpp, Plan::Step::playable), the owner's 2026-11-15 rule "so a part that can be played is never cold". `all` is the issue's behavior. The default was NOT flipped: that is the owner's call, after the FANTOM measures `all` against unset.
- Exactness: worker-rest-diff renders one session with all, unset and 0 in processes of their own and compares every node's output bit for bit (notes after silence and around the 192-block hold, the block after a release tail, parameter sets and a panic while asleep, a plugin waking itself through request_process from its main thread, the transport starting, crossing a bar, stopping). It catches a rest that ignores the wake word (checked by mutation). One real gap found and fixed: TransportWatch did not count rested frames, so a playing transport looked moved on the next called block (kick-clap-host now checks it).
- Mac worker-wake (instrument + 5 sleeping effects, each in its own worker, playable, 3 alternating runs): idle block 78-95 us unset, 3-8 us all, 67-90 us with 0; the waking block 93-115 us unset, 113-127 all (+12-20 us, the cold first block), no block lost. oscairn-rest-bench (5 voice-fixture parts + inserts; the voice fixture never sleeps, so only the 5 inserts rest): engine thread 65-92 us/block unset, 47-59 all; syscalls 26.9 -> 20.1 a block.
- The suspects for the rest excess, counted with callgrind in the aarch64 container (rest bench, 1,925 blocks): importance() asked of every budgeted worker is about 25 instructions a worker a block (WorkerHost::importanceReport returns at once with no report fresh): ruled out. The per-block timing records (BlockTiming::record, five a called worker) about 230 instructions a worker, the steady_clock reads about 600 (vDSO on arm64): together about 950 instructions of engine thread per called idle worker (disabled 58,422 vs all 53,652 instructions of rust_core_run_block a block for 5 rested), about 1 us on the A53, against the 35 us the unit sees per worker: the round trip's cost is the two scheduler wake-ups, not instructions. The worker's report writes after a block (statistics stores, voice report) are about 50-80 instructions on the worker's thread. The biggest item on an idle worker's own thread was the host's asleep check reading its input with a per-sample early exit (half of 2,650 instructions a block); now a vectorized fold, 1,650.
## Tried and ruled out
- A skip separate from rest that would not make the first note cold: there is no such thing; not calling the worker is what lets its thread park and its caches go cold. Keep-warm by ringing was ruled out before (2026-11-15 learnings).
## Watch out
- CONTINUE_IF_NOT_QUIET and TAIL plugins are never treated as asleep (only CLAP_PROCESS_SLEEP): such a plugin is called every block whatever the policy.
- A harmony that changes and changes back while a plugin sleeps reaches it once more with 0 (the host-skip path forgets the last harmony) than with rest; no sound differs.
- With chain placement (OSCAIRN_WORKERS=chain, plugin groups) a part's chain is one worker, so the cold first block is paid once a part, not once an effect: measure `all` on the FANTOM in that configuration, ST Final at rest and a note after 1 s of silence while another part plays.
