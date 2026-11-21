---
title: "Aftertouch: external pressure is a note expression at the route; input.aftertouch folds in Session::process"
author: Jacob
date: 2026-11-21
branch: wheels-2
cost: { minutes: 2432, tokens: 8636148 }
id: l-466c6947
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/route.rs
  - "core/control/src/session.cpp#Session::foldPressure"
  - core/control/src/controller_mapping.cpp
  - core/control/src/physical_controls.cpp
---

## Decided
- A keyboard's channel and key pressure never reach the routes as MIDI: MidiBlock::append runs midiToClap, which makes the CLAP pressure note expression (key -1 for channel pressure). A per-part receive filter keyed on MIDI 0xD0 alone (the wheels branch's first version) never saw them; route.rs filters the pressure expression by Function::Aftertouch too.
- input.aftertouch "channel" (the default) folds key pressure into channel pressure in Session::process, on the merged input before the routes (Session::foldPressure: the highest of the channel's keys, sent on change, a note's end clears its key). Not in the zones: a per-zone flag would have needed every part zoned (inputZones adds a pass-all zone whenever a flag is set) or would have changed the engine-core vectors, whose zones default to 0; graph-level tests (engine pressure) still see key pressure per note.
- passes is allowed on the Performance's own assignment (part 0) too: the owner's example (the mod wheel set to a part's volume, "unless configured to do both") is a partLevel assignment, which only part 0 can hold.
- A switch's latch is a property of the control (ResolvedControl::momentary from AssignmentSource::momentary); the document key is physicalControls, because the Performance's "controls" already holds its macros.

## Watch out
- Before this, key pressure always passed per note; the default now folds it for sources that send it (pads set to Poly pressure included). Poly brings the old behavior back per Performance.
- The fold keeps one table across sources and ports (a shortcut noted in the code).
