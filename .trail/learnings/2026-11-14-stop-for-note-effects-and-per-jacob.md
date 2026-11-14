---
title: STOP for note effects and per-part pedal receive
author: Jacob
date: 2026-11-14
branch: stop-and-pedals
cost: { minutes: 6, tokens: 30263 }
id: l-1baf2fb2
scope: repo
status: active
src: agent
touches:
  - "core/control/src/session.cpp#Session::stopNoteEffects"
  - core/engine/include/oscairn/note_zone.h
  - core/engine/src/NoteRoute.h
  - core/control/src/controller_input.cpp
---

## Decided
- STOP (any setPlaying(false)) sends each note effect (a node whose note output a note wire takes on) MIDI CC 123 on all channels through PluginHost::sendEvent, past NoteRoute, at the next Session::service, whether or not the transport was playing. Why: the arpeggiator only ends a latched arpeggio on a playing-to-stopped edge, and on the FANTOM the transport is never playing while an arpeggio runs free. CC 123 means release (the owner: notes ring out, Hold stays armed); the all-notes choke and CC 120 stay the cuts (Session::StopNoteEffects::Cut).
- A part's pedal receive (input `receive`, NoteZone::pedals: all / none / own per class) is data on the session side; the delivery (drop by mode, the largest value over the sources a route takes, so any pedal down holds) belongs in NoteRoute. NoteZones::takesPedal is the rule. Why: the pedal must be filtered per part before the plugin (instruments sustain in the plugin, per channel), and only the note router splits input events per part.

## Tried and ruled out
- Sending CC 123 or a wildcard choke through the graph input: NoteRoute turns them into per-slot releases of what the route holds, so they never reach a note effect as an all-notes message.
- Host::panic on the arpeggiator alone: reset() forgets its sounding notes without note-offs, leaving the synth after it stuck.

## Watch out
- NoteZones::passes checks a pedal's source against the zones' devices, which is "own" behavior: the default "all" needs the router to skip that check for pedals.
- graphState() reserves revisions in the data folder (Session::nextInfoRevision): a test that does not set OSCAIRN_DATA_HOME writes parameter-revision into the real ~/Library/Application Support/Oscairn.
