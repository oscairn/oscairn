---
title: "STOP ends held arpeggios by releasing their notes, leaves Hold armed, and reaches arpeggiators only"
author: Jacob
date: 2026-11-14
branch: main
cost: { minutes: 74, tokens: 190672 }
id: l-0eea4946
scope: repo
status: active
src: agent
touches:
  - docs/fantom-0.md
  - hosts/standalone/CMakeLists.txt
  - hosts/standalone/Catalog.cpp
  - hosts/standalone/Catalog.h
  - hosts/standalone/Engine.cpp
  - hosts/standalone/Engine.h
  - hosts/standalone/Launcher.cpp
  - hosts/standalone/Launcher.h
  - hosts/standalone/LibraryPane.cpp
  - hosts/standalone/LibraryPane.h
---

## Decided
- The owner's rule (2026-11-15), standing in for the reference manual we do not have: pressing STOP ends a latched arpeggio whether or not the transport was playing; the arpeggio's sounding notes ring out through their own release and are not cut; Arp Hold stays on, so the next key latches again.
- STOP sends note-effect nodes (arpeggiators) CC123, All Notes Off, meaning "stop generating and release". Panic sends the all-notes choke or CC120, meaning "cut". The transport's playing-to-stopped edge behaves like STOP.
- STOP's message goes to note effects only, not to instruments, so notes the player is holding on other parts are not released.

## Tried and ruled out
- Relying on the transport stopping: on a keyboard the transport is usually not playing while an arpeggio runs, so there is no playing-to-stopped edge and STOP did nothing.
- Sending the existing all-notes choke on STOP: the arpeggiator forwards a choke downstream, which cuts the notes; the owner wants them to ring out.

## Watch out
- The arpeggiator plugin (another repository) cuts on CC123 until its version 0.1.8; the host side is right before the plugin is.
- What STOP does to a recording in progress is not decided.

In the code: the panel map entry for STOP in devices/roland-fantom0/panels/fantom-07.json; the session's transport and controller handling in core/control; docs/plugin-porting.md for the note-effect contract; branch stop-and-pedals.
