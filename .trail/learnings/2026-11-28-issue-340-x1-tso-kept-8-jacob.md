---
title: "Issue 340 x1: TSO kept 8 of 19 because its silent effects were neither held nor taken over"
author: Jacob
date: 2026-11-28
branch: reload-effects-2
cost: { minutes: 5086, tokens: 25196129 }
id: l-fcb9bee7
scope: repo
status: active
src: agent
touches:
  - "core/control/src/performance.cpp#Session::holding"
  - core/control/tests/performance_switch.cpp
  - core/control/CMakeLists.txt
---

decided: FANTOM x1 (build ed66aa4f) kept 8 of TSO's 19 nodes at every pedaled reload and loaded 11 afresh. engine.timing of those loads says why: the pedaled chord sounds in Init Normal (the VA) alone; the five Samplers, their compressors and EQs, and the Reverb and Variation buses (REV-X Hall, Tremolo) were asleep and rested (called 193, rested 67, asleep true). holding() leaves anything silent three seconds to take (which compares it with the file and resets it: FANTOM k8's reason), but an engine offering effect contexts is never taken over (freeInstances, slice 1), so every silent effect loaded anew at every reload, plain reloads included (loaded 13 there). The 8 kept were the 5 silent Samplers (taken over and reset, counted in open.kept), the VA and its two effects (G Chorus, SPX Hall, sounding). Not the cause: chain placement or shared engines: each TSO effect is an engine of its own, and the VA's two effects share one chain process (w10) and were kept. Now holding keeps an effect engine silent or not (still alone in its group, opened from this very file, nothing edited); a silent instrument is still left to take. Test: control-performance-switch-steps 'silent effects', a part held through an insert and a send to a bus, a second part silent through its own insert, parts apart: reused 5, loaded 0, nothing ringing out, 0 samples differ against no reload; before, the silent insert rang out beside a context of the old Delay (1 ringing out). A -chain variant runs the steps with OSCAIRN_WORKERS=chain and the Basic Synth one block late (the FANTOM's placement and extra block). watch out: on the device the silent effects loaded as new processes, here they became contexts of the old Delay engine (the test Delay has room for contexts, the effects bundle apparently not), so the test fails by 'ringing out beside', not by 'loaded'. callbackMax in x1 read about 1.8 ms in all three phases (read 1801-1807, publish 1796-1855, after 1648-1873 us): the publish no longer stands out, so warm-up blocks before the publish are not indicated; with this change no effect is fresh at a same-Performance reload.
