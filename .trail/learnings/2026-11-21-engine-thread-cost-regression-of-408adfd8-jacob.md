---
title: "Engine-thread cost regression of 408adfd8: per-block slot scans, an eager importance merge, and a bus fed by nothing costing a second round trip"
author: Jacob
date: 2026-11-21
branch: engine-cost
cost: { minutes: 2571, tokens: 8737295 }
id: l-c7638c0e
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/route.rs
  - core/engine/rust/src/budget.rs
  - core/engine/rust/src/notes.rs
  - core/engine/rust/src/audio.rs
  - core/engine/src/graph.cpp
  - core/plugin-host/src/WorkerHost.cpp
  - core/clap-host/src/Host.cpp
  - core/clap-host/rust/src/host.rs
---

## Decided
- The unit's +10 % engine-thread CPU on 408adfd8 (notes/w27/cost-2026-11-21.md: heavy 946 -> 1043 us/block, rest 390 -> 426) had two causes, both now fixed; the rule behind both: a guard that is not acting does no per-block work beyond reading a count.
- 1. NoteRoute::voiced (488775c1, held keys whose voices ended left out) walked every used slot and read each Slot every call; the budget calls it per lane (estimate), per entry (recover's node_notes, O(entries^2) a block), per step (finish, voice_limit). Now an `ended` bit set kept on NOTE_END and cleared in drop_slot/clear, voiced() = used & !ended, a popcount as before the range. recover counts a node's notes only for an entry it caps.
- 2. hear() ranked every reporting context's 16 entries by insertion (partition_point + copy_within each) and copied a whole Heard every block. Now the entries are kept as listed (Heard::take) and ranked by Heard::rank only when choose_on runs, once per report; the same insertion in the same order, so the ranking is bit-identical (engine-core.vec replays unchanged). Audio::reach (the mix gains for the ranking) recomputes only when a live gain its routes read moves (slots listed in Audio::new, not on the audio thread). WorkerHost copies the importance report out of the shared block only when the core asks (importanceReport, after finish and before the next start; start() clears the flag), and only `count` entries.
- 3. The extra round trip: 7fa528f7 leaves a send at level 0 out of the plan, but the document's wire still places the bus with the part's inserts (placementGroups). Fed by nothing, the bus was its own wave (wave 0), so the insert worker woke twice a block: ST Final's 4-effect worker 752 wakes/s against 375, engine FIFO wakes +280/s, the kernel unlock and startProcess memmove growth. Graph::compile now gives a groupable node with no audio, note or mapping edge into it a floor at the earliest later wave where a fed node of its own process renders (plugin-groups test: two such buses grouped).
- 4. The CLAP hosts (C++ and Rust) ask draft 2's importance only after a block with active_voices != 0 (no voice sounding: no context sounding, the report is empty): the SDK helper's list() over 128 voices is saved at rest, on the worker's thread.
- Mac, oscairn-core-bench (8 lanes, draft 2, 120 held): core 0.717 us/block at b7a5e3fc, 1.571 at 77397e83, 0.753 after; 240 held 0.763 / 1.919 / 0.818. oscairn-session-bench (chain, 4 pianos x 60 held + lead, 8 alternating runs, min of each run's median): rest 42.5 / 49.7 / 45.2, held 62.5 / 70.8 / 59.0 us.

## Tried and ruled out
- Reading the importance lists lazily through the plugin callback at choose time: core_importance.cpp hands the lists in finish from a stack array, and a shared block is rewritten by the worker once its next block starts; the core must take the entries at finish.
- Building the worker's report only when the host asks (a "report wanted" hint): an SDK change; skipping it at active_voices 0 is the only saving the contract allows now (an SDK hint would let a plugin skip list() while no lane is near its limit, at the cost of a report a block late when one gets there).

## Watch out
- The extra worker processes (ST Final 6 against 5, Sick tines 10 against 7) are instances kept warm (RFC 0021's middle rule in Session::readTaking: a seamless change keeps playing instances warm instead of taking them over); 19bfba11's per-core seamless rule makes more changes seamless. They are not in the plan (FIFO 0 wakeups) and cost the engine thread nothing; a kept-warm Sampler's own I/O thread still wakes about 900 times a second.
- Thread CPU time on this Mac is bimodal (P and E cores, other agents' tests): compare many alternating runs by their minimum, never one run.
