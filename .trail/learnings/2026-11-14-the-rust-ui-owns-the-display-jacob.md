---
title: The Rust UI owns the display and hands it over for JUCE screens
author: Jacob
date: 2026-11-14
branch: rust-ui
cost: { minutes: 13, tokens: 33319 }
id: l-9f237809
scope: repo
status: active
src: agent
touches:
  - ui/src/shell.rs
  - ui/src/tests.rs
  - devices/roland-fantom0/bundle/oscairn-fantom-start
  - ui/src/nav.rs
---

Decided (owner, 2026-11-14): the UI moves to Rust screen by screen. Only one program can own the display without X, so oscairn-ui owns it (Slint linuxkms) and, for a screen not yet ported, exits with status 3 naming it; the start script's keep_ui runs Xorg and the JUCE app with --connect and OSCAIRN_HOME_QUITS=1, then starts oscairn-ui again. The engine runs alone (engine-only start) so neither UI touches audio. Ruled out: both under X until all is ported (about 650 misses scrolling under X against 0 without); keeping the shell alive while dropping DRM master (the backend cannot, and a restart is under a second). Panel navigation comes from the engine's controllers navigation totals (main's navigate), not a reader of the panel holder in the UI: one decoder, in the engine. Watch out: the shell's tests must set OSCAIRN_DATA_HOME and OSCAIRN_CONFIG_HOME before anything reads them (tests.rs Screen::new); before that a Setlist test overwrote the user's real AppData/oscairn.setlist/state.json. Within one controllers push, moves must be applied before confirms (Totals sorts them), or a turn-then-press confirms the old item.
