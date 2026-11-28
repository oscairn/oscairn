---
title: "oscairn-plugin-check compares a note effect by the MIDI it sends, and moves states from a session of its own"
author: Jacob
date: 2026-11-28
branch: moves-183
cost: { minutes: 4111, tokens: 23661217 }
id: l-beb8405f
scope: repo
status: active
src: agent
touches:
  - "core/control/tests/plugin_check.cpp#stateFrom"
---

Decided: a note effect (the note-effect feature, or notes in and out with no audio output) is compared by every MIDI message its part sends out (block and bytes), in the edit check and in --state-from, because an arpeggiator makes no sound and the audio comparison failed on every real run. --state-from moves each state from a Session of its own, because a library move of the open Performance is refused (saving it would write the old versions over the move). Watch out: the plugins session's 'the store says the open Performance plays it (used by 1)' failure was not reproduced here; the check now finds the plugin's own entry among the store's, since a bundle lists each plugin it holds, and says whether the entry was missing or not open, so the next real run tells which it is.
