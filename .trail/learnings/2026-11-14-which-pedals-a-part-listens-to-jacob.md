---
title: Which pedals a part listens to is separate from where its notes come from
author: Jacob
date: 2026-11-14
branch: main
cost: { minutes: 19, tokens: 21594 }
id: l-6bc66aff
scope: repo
status: active
src: agent
touches:
  - core/control/include/oscairn/parts.h
  - docs/performance-format.md
  - devices/roland-fantom0/panels/fantom-07.json
---

## Decided
- The owner's requirement (2026-11-15): a pedal plugged into the instrument must be able to sustain a part whose notes come from a second, external keyboard. On the reference instrument he has to move the pedal to the second keyboard for that, and he wants that limitation gone. So a part's note input devices must not decide which pedals reach it.
- Parts also carry the reference's per-part "receive sustain / sostenuto" switch, so some parts ignore the pedal (in his ST Final: Bass, Pad 2 and Sweep ignore it; Pad and the arpeggiated part receive it).
- The filter is applied by the host on the part's input, before the plugin, because the engines hold notes per MIDI channel inside the plugin and several parts can share one instance.

## Tried and ruled out
- Making sustain a control assignment like a knob: sustain has to reach any plugin, third-party ones included, as an ordinary pedal message with half-pedal values.
- A parameter in each engine for "ignore the pedal": it would not cover third-party plugins and cannot work per part on a shared instance.

## Watch out
- Proposed to the owner, not yet confirmed by him: by default every part listens to every pedal; per part the choice is all pedals, none, or only the pedals of the devices its notes come from; with several pedals a part is held while any is down. Check his answer before treating the default as settled.
- A pedal jack given another job (a free control) stops being a sustain pedal; which jacks exist is per device, the behavior is the same everywhere.

In the code: a part's input settings in core/control/include/oscairn/parts.h and docs/performance-format.md; per-route delivery of controller events in the engine core (core/engine); the panel map's pedal entries in devices/roland-fantom0/panels/fantom-07.json; branch stop-and-pedals.
