---
title: "voice-importance-scenario: the piano's claim counts only the rules that take across lanes"
author: Jacob
date: 2026-11-21
branch: scenario-ci
cost: { minutes: 1562, tokens: 7293420 }
id: l-45101ca5
scope: repo
status: active
replaces: l-5a9a0d29
src: agent
touches:
  - core/engine/tests/voice_scenario.cpp
  - docs/developer-guide.md
---

## Decided
- 'the piano, on another lane, loses nothing for the lead's' (voice-importance-scenario, lead's own lane over) now counts only the voices the rules that may take across lanes gave up on the piano (NoteCost.stolenForDevice + stolenForLoss + stolenForDeadline), never stolen/hardChokes (its own lane's limit), and is checked only when neither lane was held off a core (heldOff 0 on both) and neither part lost a block. Those rules take from any lane only when the device's time is judged short (Budget::device on a held lane or the lanes' total, lost() on a held lane, deadline() with device_over), which a slow machine can cause by itself.
- Why: GitHub's macos-15 job at 408adfd8 failed it in pool, two processes: piano 30 at the lead's entry, 28 at the end, lead lost 2 blocks and gave up 4 (one held key), heldOff 0. The old premise (piano kept 30 until block 99) did not cover a give-up after the lead's entry; the output did not say which rule. The test now prints 'piano gave up N for its own lane, M across lanes; held off a core: ...' for that case.
- The earlier premise pianoBefore==30 (TSAN: the piano's own lane shed 10-11 before the lead played) is dropped: own-lane give-ups are no longer counted at all.
## Ruled out
- Product fault, as far as tested: Mac 30 unloaded + 30 beside 40 busy loops, 2-CPU Linux container 20, TSAN 10: piano across-lanes 0 in every checked run. A mutation (Budget::device takes one voice from any lane whenever a lane is over its limit and not held) gives 'piano gave up 0 for its own lane, 2 across lanes' and fails all three apart placements, the same 30>28 shape as CI.
## Watch out
- With the lead losing blocks on a slow runner the claim is now not checked; the CI line will say so. If it ever fails again, the printed line names the rule.
