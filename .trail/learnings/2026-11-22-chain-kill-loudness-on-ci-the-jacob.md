---
title: "Chain-kill loudness on CI: the instrument missed a block before the compared note, nothing unrestored"
author: Jacob
date: 2026-11-22
branch: chain-kill
cost: { minutes: 90, tokens: 65508 }
id: l-7654c6a6
scope: repo
status: active
src: agent
touches:
  - "core/engine/tests/plugin_groups.cpp#killOneChain"
---

decided: plugin-groups-rest-all's 'chain kill: part 3 as loud as before the kill' failing on the macOS runner with 0.073072 before, 0.064528 after (run 44958034411, 4b5e01b4) was not a chain restored without its state and not ringout-4's WorkerHost changes. The shared Basic Synth's worker (never killed) missed one block in the cycle before the compared note, outside the relaxed excuse (a block either side of the compared, held blocks only). A block the instrument misses it never plays, so the note before starts a block nearer the compared strike, its release tail sits under the new note in another phase, and the peak of their sum moves: the first cycle, with no tail, peaks 0.0669 against 0.0731 later. Evidence: SIGSTOP of the synth worker for 3.5 ms at block 380 (again = 400) gave exactly 0.064528 with every gain restored; 6 ms (two blocks) 0.066363, three 0.067264, five 0.070384; a stall after the strike reads louder (0.075054). The same 6 ms stall gives 0.066363 on ced2ca13, before ringout-4. Unstalled, 0 of 30 under 40 busy loops with CI set on each of main and ced2ca13 on the Mac: its restart is back within 50 blocks and the synth misses nothing. Relaxed, the loudness is now compared only if the instrument (fm0 and part 3's sound context) also missed no block before block 300 or from the strike before the compared one on; a second run stalls the synth worker 6 ms as part 3 comes back and checks the misses are seen. ruled out: a default gain (the fixture's is .5, so one insert at its default would give 0.0609, 0.0522, 0.0457 or 0.0406, never 0.0645); measuring too early (the stretch compared is the first whole note after the chain is back). watch out: the chain's own misses cannot use the long window, the kill itself is counted as misses on its plugins. Off CI the comparison stays exact, so a loaded dev machine that stalls the synth still fails it. A Tines workerPid of 0 passed to kill() stops the whole process group (test and shell): guard pid > 0.
