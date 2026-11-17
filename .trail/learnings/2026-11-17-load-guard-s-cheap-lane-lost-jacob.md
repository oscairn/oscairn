---
title: "load-guard's cheap lane lost notes: a late instrument's lane was not late, and its cost per note was paired with the wrong block"
author: Jacob
date: 2026-11-17
branch: flaky-two-b
cost: { minutes: 120, tokens: 3217325 }
id: l-df5199b3
scope: repo
status: active
src: agent
touches:
  - core/engine/src/PreparedInstrument.cpp
  - core/engine/rust/src/notes.rs
  - core/engine/tests/load_guard.cpp
  - core/engine/tests/vectors/engine-core.vec
  - core/engine/tests/vectors/engine-run.vec
---

## Decided
load-guard's device case ("the cheap lane loses nothing for the lanes' total", B lost 1-2) failed alone 25 of 30 on the Mac and on the ubuntu runner: product faults, not noise. Diagnosed by printing both nodes' NoteCost and oneBlockLate() per run.
1. PreparedInstrument (every instrument added with Graph::add renders through it) did not forward oneBlockLate(), so graph.cpp never marked A's lane late: its limit was 800 us (allowance) not 2,000 (a period); A faded 3-4 notes for its own lane, kept its last, and device() could only fade B. Fixed: it answers its first instance's, cached at construction.
2. Then B still lost 1 of 3: a late plugin's block time was observed against this block's notes, but it rendered the previous block's, so the first note measured 0 us, and the held-up rule (took > 2x expected counts as expected) kept the estimate near 0 for ~12 blocks (A 1-9 us/note vs B 53-70 when the total ran over). Notes::finish now counts a late lane's plugin by Step::live_before.
Change 2 alters engine-core-diff and engine-run-diff; their vectors were recorded again from the Rust core at 022702d9 (the first not from the C++ reference; needs the owner's review).

## Watch out
- step.late_lane is lane-wide (every host on the lane late); a late host on a mixed lane is still paired with this block's notes.
- The 2026-11-17 learning that blamed the machine for the device claim's failures was right for its lost-block runs, but the "lost 1" with no lost blocks was this fault.
