---
title: "Issue 340: a same-Performance reload kept its instruments but loaded every effect afresh"
author: Jacob
date: 2026-11-28
branch: reload-effects
cost: { minutes: 4923, tokens: 24951524 }
id: l-734f5a92
scope: repo
status: active
src: agent
touches:
  - "core/control/src/performance.cpp#Session::holding"
  - "core/control/src/performance.cpp#Session::read"
  - "core/control/src/session.cpp#Session::blockTimes"
  - core/control/tests/performance_switch.cpp
---

decided: FANTOM d1/w1's late and silent blocks on pedaled TSO -> TSO reloads (6% and 5 of 50; the row always a fresh effect last in chain order, its own round trip ordinary, the engine callback at 2.36-2.47 of 2.67 ms) had this behind them in the code: Session::holding never kept an engine offering effect contexts ('engines offering effect contexts go by slice 1'), so the reload kept its 6 instruments as they were but loaded all 8 effects anew (or as contexts of the old engines), and the old effects stayed in leaving_ ringing out their tails beside the new ones fed by the sounding instruments: twice the effect workers called per block from the publish on, on 3 worker cores. In a plain reload both sets get silence and rest, which is why only held notes showed it. Now holding keeps an effect engine when it is alone in its group (graph_.effectSource(n,{n})==n) and its loaded_ source is this very file (folder@stamp), not merely a state of the same bytes: a file saved again may wire it otherwise (chain order, sends), and then it behaves as before. unfed() already drops a kept node an outgoing non-kept sounding node feeds (an instrument not kept would otherwise lose its effect's tail). read()'s playing() (contexts of a playing engine) skips holders, so a kept engine stays alone and is kept again next time. Test: control-performance-switch-steps 'kept effects' renders block by block on the test thread a load, a held key, a reload, 300 blocks, against the same without the reload: 0 of 38400 samples differ (7532 differed before, the Delay a fresh context, the old one ringing out), kept 2, loaded 0, leaving 0; also under OSCAIRN_WORKERS=chain. engine.timing's open gained callbackMax {read, publish, after}: the callback's longest block per phase of the last load (Session::openPhase_, set by openPerformance, read by blockTimes on the audio thread; 'after' is the 512 blocks after the publish, ended by the audio thread). ruled out for now: warm-up blocks for fresh effect workers before publish (RFC 0045 step 10): with the keep, the measured case has no fresh effect; the effects bundle's process returns early while asleep on silent input (OscairnEffects.cpp asleep), so warm-up needs input it cannot sleep through and a reset after, and it costs serial round trips on every load; left until the device shows fresh effects still lose blocks. watch out: an effect that became a context of another engine at an earlier load (not alone) is still not kept; the device's TSO effects were roots, one process each (8 started per reload), so the keep should apply there, unverified.
