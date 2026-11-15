---
title: "A scene that covers on/off is a full snapshot: it switches on every connection it does not list as off"
author: Jacob
date: 2026-11-15
branch: main
cost: { minutes: 1, tokens: 5715 }
id: l-95953dc7
scope: repo
status: active
src: agent
touches:
  - core/control/src/parts.cpp
---

## Decided
- A scene whose scope includes Enabled sets every wire's enabled state to "not in the scene's offWires". It is a snapshot, not a delta, so a wire that is disabled in the base Performance is switched ON by any such scene that does not list it.
- Whoever writes scenes (the converter, an importer, the UI's capture) must either leave Enabled out of the scope of a scene that should not change on/off, or list every wire that must stay off. Capture already does this by listing every disabled wire.

## Watch out
- Found from a converted Performance where a part is off the keyboard at load and only some scenes memorize that switch: the scenes that memorize nothing must not carry Enabled in their scope, or selecting them turns the part on.
- Read from the code, not run against a converted Performance.

In the code: `applyScene` and `capture` in `core/control/src/parts.cpp`; `Scene::offWires`; scope kind `Scene::Enabled`.
