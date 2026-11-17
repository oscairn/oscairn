---
title: "A part's original sound selected back on a shared instance shows the values the node left it with"
author: Jacob
date: 2026-11-17
branch: unit-findings-3
cost: { minutes: 1016, tokens: 1618937 }
id: l-e3521603
scope: repo
status: active
src: agent
touches:
  - core/control/include/oscairn/busiest_load.h
  - core/control/include/oscairn/control.h
  - core/control/include/oscairn/session.h
  - core/control/src/budget.cpp
  - core/control/src/performance.cpp
  - core/control/tests/admission.cpp
  - docs/control-protocol.md
  - docs/rfcs/0025-resource-budgets.md
  - core/control/tests/value_reports.cpp
  - core/engine/include/oscairn/sound_context.h
---

## Decided
- SoundContext::selectSound (core/engine/src/sound_context.cpp) shows a sound's values from the plugin's sound_values. When the plugin gives none, it shows the values this node had when it left that sound. They are kept in left_ (sound_context.h) until the sound is discarded or selected again.
- Why: sound-snapshots draft 4 returns nothing for a sound any MIDI channel selects. An instance's first sound (context 0) stays selected on every channel no part uses. So a part going A -> B -> A got nothing back for A. It kept showing B's values, and the control thread, comparing after valueRevision moved, saw no change and published nothing. On the unit this was 148 stale values in Sick tines' French Tines 1.
## Tried and ruled out
- Blaming 919be035 (values compared only when valueRevision moves): the same showValues ran before it. The revision did move. The values compared were simply still B's.
- Asking the plugin for a selected sound's values: the extension forbids reading them (the audio thread changes selected sounds' patches).
## Watch out
- left_ is only right while no other part's channel edits the same sound. Unused channels get no events, so the first sound changes only through this node.
