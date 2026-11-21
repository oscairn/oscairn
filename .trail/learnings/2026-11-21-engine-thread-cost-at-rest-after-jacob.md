---
title: "Engine-thread cost at rest after the first fix: report scans with no report held, worker stamps asked every block, a report count on its own line"
author: Jacob
date: 2026-11-21
branch: engine-rest
cost: { minutes: 2648, tokens: 10519534 }
id: l-415f593e
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/budget.rs
  - core/engine/rust/src/notes.rs
  - core/engine/rust/src/run.rs
  - core/engine/rust/src/work.rs
  - core/plugin-host/src/WorkerHost.cpp
  - core/plugin-host/src/worker_shared.h
  - core/engine/tests/cost_budget.cpp
  - tools/engine-bench/rest_bench.cpp
---

## Decided
- The rest of the unit's at-rest regression (notes/w27/cost2-2026-11-21.md: ST Final 380 -> 402 us/block on 30781e0c) was measured, not guessed, with instruction counts: the rest bench (tools/engine-bench/rest_bench.cpp, 5 voice-fixture instruments each through an insert, chain, budget on) under callgrind in the aarch64 Linux container, `--collect-atstart=no --toggle-collect=rust_core_run_block`. Counts repeat to a few instructions; this Mac's thread times do not (bimodal P/E cores: 8 alternating runs of b7a5e3fc spread 39-58 us/block p50). Per block: b7a5e3fc 57,040, 30781e0c 61,006 (+3,966), this branch 58,178 (+1,138).
- 1. Passes over the budget's entries for a held importance report though no entry held one: hear() every budgeted step every block, context_cap/step_cap per step and context, voice_limit's listed scan, estimate's voice_cost per entry (~1,500 instructions). Budget::listing counts the entries holding a report (kept where hear takes or lets go of one); with 0 none of these passes runs.
- 2. RFC 0046 step 4's worker stamps (Plugins::waited -> PluginHost::blockTimes, through PreparedInstrument's instances) asked for every rendered step every block (~700): judge_held reads them only on a lane in trouble (over its limit or a block of its own lost). Notes::time_lanes asks them after the finishes for those lanes only and sums them again in plan order, so the lanes come out bit for bit as before (stamps_asked_late_judge_the_same). run_diff's table has no `waited`, so engine-run.vec is untouched.
- 3. The importance report's count sat on a shared-block line of its own (after the 512 voice notes), written by the worker and read by takeImportance every block (a coherence miss per budgeted worker on the A53, invisible to callgrind): moved beside activeVoices, and finish marks the report fresh only when the count is not 0. bindings.rs regenerated (MADE_FROM).
- 4. Small: Run::finish zeroed a 16-word IDs array per step per block (now the run's scratch); workerTimes clamped two stamps every finish (now when read).
- Instances kept warm cost the engine thread nothing: callgrind per block equal with and without one (90,217k vs 90,233k over the run), the engine thread's context switches per block equal (7.06 vs 7.04, Linux), engine-cost-budget's counts 0. The kept-warm Sampler's 928 wakes a second are the Sampler's own thread: plugins/sampler src/streamer.cpp:108, `osc-sampler-io` waits `wait_for(1 ms)` whenever no stream is active, in every Sampler process, playing or not (the active one wakes as often). Not a host loop; the plugins session's to make it block until a stream starts.
- engine-cost-budget (C++, cost_budget.cpp) and engine-cost-budget-core (Rust, run.rs cost_budget) hold the per-block work to recorded budgets by counting it (docs/developer-guide.md, "Per-block cost budgets"); each fails on 30781e0c's code (checked for the core by putting back one scan: entries 136 -> 200 per block, over its budget).

## Tried and ruled out
- Skipping p.importance(s) when the plugin reports no voices: run_diff's fake table logs every callback and moves its clock by each log, so engine-run.vec would no longer replay. The call stays; it costs ~50 instructions a budgeted step through the C++ chain.
- Counting the engine side's work with counters always compiled in (WorkerTiming::add in WorkerHost): +700 instructions per block at rest (the add was not even inlined). They are compiled only into oscairn_plugin_host_counted and oscairn_engine_counted, which only the test links (OSCAIRN_COST_COUNTERS); the core's counters are cfg(test).
- Callgrind toggled on Graph::process's long C++ signature: its counts did not cover the block (32M against 88M with rust_core_run_block); the per-block C++ outside the core did not change between the two commits anyway (graph.cpp's diff is plan-time code and two table entries).

## Watch out
- +1,138 instructions per block remain on the 10-step rest bench (Notes::finish's lane stamps and voiced() counts, Audio::reach's slot check, the importance callback chain, route's frame_us), about 1 us on the A53. The fix removes ~2,800 instructions per 10 steps, perhaps 4-6 us/block for ST Final: the unit's +22 us may not all be instructions (more worker processes with an I/O thread polling at 1 kHz on four cores; the shared block's hot lines). Re-measure on the keyboard, and ST Final at rest once loaded fresh (no instance kept warm) to separate the two.
