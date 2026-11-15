---
title: "HOLD jack sent CC 16, not CC 64: a map entry with no max scales on the layout's 1023"
author: Jacob
date: 2026-11-15
branch: worktree-agent-aa6651bbf6e2fb456
cost: { minutes: 29, tokens: 59622 }
id: l-ba75e848
scope: repo
status: active
src: agent
touches:
  - devices/roland-fantom0/panels/fantom-07.json
  - devices/roland-fantom0/tests/panel_reader.cpp
---

decided: the HOLD jack (analog 2, footswitch:1) reads 0 to 127, so its map entry says max 127; the engine path (panel reader, Session::controllerControl, ControllerInput::control, Session::controller as CC 64 from the built-in source) was right all along and needed no code change. Cause: an analog entry without max scales on layout.analogMax 1023, so a full press was 127/1023 and CC 16, under the 64 a plugin takes as down; injecting CC 64 worked because it skipped the scaling. ruled out: the footswitch path dropping analog events, the touched-needs-an-action rule, the profile function missing in the engine's setUpSession, a threshold the ramp never crosses, and the per-part receive filter (a part set to none stays silent, one at all holds the key). watch out: panel_reader.cpp's holdJack test feeds the shipped map and a ramp through a reader into a Session; a part's setPartInput needs s.commit() before the zones take it. The CTRL jacks (analog 3, 4) still have no max, so they read on 1023 until measured.
