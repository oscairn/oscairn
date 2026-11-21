---
title: input.aftertouch defaults to poly; channel only where a Performance writes it
author: Jacob
date: 2026-11-21
branch: wheels-3
cost: { minutes: 2865, tokens: 10870935 }
id: l-1fe689c5
scope: repo
status: active
src: agent
touches:
  - core/control/src/performance.cpp
  - core/control/include/oscairn/session.h
  - "core/control/src/session.cpp#Session::foldPressure"
  - core/control/tests/pads.cpp
---

## Decided
- The owner decided (2026-11-21, on the wheels branch before its merge): poly aftertouch must work with any controller or keyboard that has it, so the host's default for a Performance's input.aftertouch is poly. Session::polyAftertouch_ starts true, the reader in performance.cpp takes a Performance without the field as poly, and the writer stores only "input":{"aftertouch":"channel"} (Performances store deviations only). Session::foldPressure runs only for a Performance that says channel.
- This takes over the 'channel (the default)' line of the 2026-11-21 aftertouch learning; the rest of that learning (pressure is a note expression at the route, the fold lives in Session::process and not in the zones) still holds.
- A keybed with channel pressure only plays the same under either mode: midiToClap makes channel pressure the pressure expression with key -1 and the fold leaves that alone.

## Ruled out
- Poly as an opt-in setting per Performance (the branch's first version): a poly keyboard would then lose per-key pressure in every Performance not edited for it, which the owner refused.

## Watch out
- Performance version stays 13 and the field's two values are unchanged, but a file written by the earlier wheels branch with no input field was channel there and is poly now; pre-release formats break freely.
- control-pads' file checks look for "input":{"aftertouch" and not for "input" alone: every part has an input object too.
