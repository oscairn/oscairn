---
title: "Wheels and lever: one logical pitch and mod control, extra inputs follow them, sprung inputs take over on crossing"
author: Jacob
date: 2026-11-17
branch: wheels
cost: { minutes: 922, tokens: 1392598 }
id: l-fdc72f5b
scope: repo
status: active
src: agent
touches:
  - core/control/src/controller_input.cpp
  - core/control/src/physical_controls.cpp
  - hosts/standalone/profiles/fantom-07.json
  - devices/roland-fantom0/panels/fantom-07.json
---

## Decided
- The owner meant the two WHEELS (WHEEL1, WHEEL2 on the panel), not only the lever. One logical pitch control (wheel:1) and one logical modulation control (wheel:2) are what Performances, assignments, passes and a part's receive name; the lever's two axes are extra inputs (lever:1, lever:2) that the device profile links to them by default (profile fields follows and rest on an assign group; kinds lever, joystick, ribbon exist for other boards). A Performance unlinks an extra input with `unlinked` (assignments.link); it is then a control of its own. Nothing is specific to this keyboard in the code.
- Two inputs on one control never add. An unsprung input (no rest) takes over at once; a sprung one (rest 0.5 or 0) takes over only when it reaches or crosses the value, measured from its rest, then follows it back to rest. Summing is a user choice: unlink the lever and give it a relative assignment on the wheel's target.
- The dead zone is generic: a sprung input's position within 1% of its throw of its rest is the rest (ControllerInput conditioned()), so the lever's 512 of 1023 is exactly the middle. An unsprung control has none.
- The owner's statements: WHEEL1 is sprung, WHEEL2 is not; which analog id is the left wheel is not in the docs (5 is the test tool's WHEEL1): confirm by moving one wheel and reading controllers.get, which names the control moved last by the input's own id.

## Watch out
- Session::assign changed the assignments but the controllers' snapshot was only compiled again when something else changed: a lever assigned alone did nothing until another edit. assign() now invalidates it.
- Undo and the Performance file carry `unlinked`; a device without the control keeps the setting and any assignments, inert.
- Bend and CC 1 from a built-in control are tested in a worker and with a plugin that declares both dialects and prefers CLAP (the first-party engines do): they arrive as CLAP_EVENT_MIDI in both hosts (control-wheels, control-wheels-clap and their -workers variants; KICK_FIXTURE_PREFER_CLAP).
