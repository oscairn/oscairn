---
title: "A shared node's Output bus value was refused for good, so every save after a scene recall said edits were pending"
author: Jacob
date: 2026-11-16
branch: engine-edit-fixes
cost: { minutes: 324, tokens: 614980 }
id: l-2e661954
scope: repo
status: active
src: agent
touches:
  - core/control/src/session.cpp
  - core/engine/src/sound_context.cpp
  - core/control/tests/scene_save.cpp
---

## Decided
Session::sendParameters drops a value command that a shared node (SoundContext) refuses for its Output bus parameter (keepsBus), and publishes a resync so clients read the bus it has. SoundContext::setParameter keeps refusing a bus other than its own (core/engine/tests/shared_engine.cpp pins that).

Why: sendParameters keeps a refused command and retries it each tick (a full queue), and writeDocument (performance.save, library.save, parts.saveSound) refuses while any command waits. A permanent refusal blocks every save until a load. A scene stored with other buses (nodes at 6, 7, 8, stored 0, 1, 2) staged that value at each recall. Found with a debug line listing pending_ in the real engine: every stuck entry was parameter 8 of a Basic Synth node.

## Tried and ruled out
- Rested workers, a full command ring, values equal to the current ones, the audio thread not draining: scenes recalled with and without audio, the step sequencer and synth in workers, saved fine; the engine ran null audio and the entries stayed.
- Making SoundContext::setParameter accept the value: breaks shared_engine's "a node's bus is its place in the engine".

## Watch out
- Any other parameter a plugin refuses for good (not a full queue) would hold saves the same way.

In the code:
- core/control/src/session.cpp, Session::sendParameters and keepsBus; core/control/tests/scene_save.cpp (keptBus).
