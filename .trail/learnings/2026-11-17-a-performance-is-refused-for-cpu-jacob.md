---
title: A Performance is refused for CPU only when its plugins at rest do not fit
author: Jacob
date: 2026-11-17
branch: unit-findings-3
cost: { minutes: 908, tokens: 1395437 }
id: l-d09531b9
scope: repo
status: active
src: agent
touches:
  - core/control/tests/value_reports.cpp
  - core/engine/include/oscairn/sound_context.h
  - core/engine/src/sound_context.cpp
  - docs/rfcs/0021-sounds-performances-and-library.md
  - core/engine/tests/plugin_groups.cpp
  - docs/developer-guide.md
  - docs/fantom-0.md
  - docs/rfcs/0045-audio-scheduling-on-small-devices.md
  - hosts/standalone/profiles/fantom-07.json
  - hosts/standalone/tests/profile.cpp
---

## Decided
- Admission (Session::estimateOf in core/control/src/budget.cpp) opens a Performance with the "may not fit" notice when its CPU does not fit but each plugin's measured rest does (total and costliest chain). The load guard sheds voices for any load above rest; it cannot remove a plugin's cost at rest. The owner's rule: a false refusal on stage costs more than voices shed. Memory still errs high and refuses.
- The busiest figure (measurements.json "cpu") follows a ~1 s mean of the median of the last 5 ticks (BusiestLoad in core/control/include/oscairn/busiest_load.h), not of the raw tick. Ticks averaging more than the block period, ticks spanning over 0.25 s (control thread held by a load), and the first 2 s after a worker restart are not sampled.
## Tried and ruled out
- Tightening the impossible-figure check on read: the unit's poisoned fm2 figure (0.339 of a block, Sick tines refused at 232%) is a possible share. Even rest + voice x most voices (about 0.49 for fm2) is above it. No read-time bound tells it from a real heavy passage.
- The "uncertain" quarter rule: it only covers figures from fewer than 8,000 blocks. The poisoned figure was certain.
## Watch out
- Cost.rest is the whole cpu when no rest was measured, and for a package's estimate (estimate.json). Otherwise a plugin that never rested, or one counted at its package's figure, would never refuse (control-parts and control-archives check those refusals).
