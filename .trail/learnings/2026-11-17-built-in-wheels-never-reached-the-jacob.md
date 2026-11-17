---
title: "Built-in wheels never reached the parts: the panel fallback was pedal-only with no pitch bend"
author: Jacob
date: 2026-11-17
branch: wheels
cost: { minutes: 1126, tokens: 1795846 }
id: l-80268778
scope: repo
status: active
src: agent
touches:
  - core/control/src/controller_input.cpp
  - core/control/src/physical_controls.cpp
  - core/engine/src/NoteRoute.h
  - core/engine/rust/src/route.rs
  - docs/fantom-0.md
---

## Decided
- The cause: ControllerInput::control turned a control's function into a message only for pedal: and footswitch: controls, and its table had no pitchBend. So the FANTOM lever (wheel:1 pitchBend, wheel:2 modulation) was dropped silently. The history shows no build ever had a path for it (an external keyboard's wheels worked because their MIDI goes straight to the parts), and docs/fantom-0.md even said the panel's wheels play only what is assigned. Fixed for every control kind: any control with a function sends it from the built-in source (pitch bend 0xE0 with 8192 at rest and a 1% dead zone about the middle, because the lever rests at 512 of 1023, which is not 511.5; channel pressure for aftertouch).
- The owner's rule for the parts: a Sound's own assignment of the control (part non-zero) replaces the function for that part only, unless the assignment says passes; a Performance-level assignment (part 0) replaces it for every part. Done in the engine's zones (NoteZone::functions, two bits per controller as for pedals), applied by the host from the assignments (Session::inputFor), so the same receive filter also covers an external keyboard's wheels and a part's own receive setting (pitchBend, modulation...).

## Tried and ruled out
- Sending the function per part from the host: events reach parts only through the graph's note routes, so the filter has to be in the route (NoteRoute.h and route.rs, both).

## Watch out
- Session::assign must buildParts and syncWires when a part's effective receive changes (editWires does syncWires; buildParts alone leaves the graph as it was, which looked like the zones being ignored). Staging only on a real change keeps assignments.set from requiring a commit before save.
- NoteZone and the Rust Zone are layout twins: a new field needs the Rust struct, engine_core.h's oscairn_zone and bindings.rs MADE_FROM (the header's FNV hash; bindgen does not emit this type).
- An absolute control assigned to a parameter waits to be picked up: a test must move the control near the value first.
