---
title: Port a whole app to the Rust shell and switch it over complete; no half-ported app that hands the display back
author: Jacob
date: 2026-11-15
branch: main
cost: { minutes: 219, tokens: 748014 }
id: l-1b2d1989
scope: repo
status: active
src: agent
touches:
  - core/clap-host/rust/src/host.rs
  - core/engine/tests/recording_fixture.cpp
  - core/clap-host/CMakeLists.txt
  - core/clap-host/src/Bridge.cpp
  - core/clap-host/src/WindowBridge.cpp
  - core/clap-host/rust/src/lib.rs
  - core/clap-host/rust/src/bindings.rs
  - core/CMakeLists.txt
  - core/engine/tests/clap_host_rust.cpp
  - core/clap-host/include/kick_clap/bridge.h
---

## Decided
- A JUCE app moves to the Rust shell as a whole and is switched over only when every screen and action in it is rebuilt, so nothing inside the app hands the display to JUCE. The owner asked for this after the Play and Part screens each landed with a labelled row that opened the JUCE app for the rest; each handover costs seconds on the keyboard (a fresh X server and a fresh JUCE process every time) and needs handover code that is thrown away later.
- The Performance app is first. Other apps stay JUCE until each is ported whole.
- The one gap allowed inside a ported app is a plugin's own custom view, which waits for the host-provided surface; the host-drawn parameter pages cover every plugin meanwhile.

## Tried and ruled out
- Porting one screen at a time with a handover row for what is left (Play, then Part): it worked, but every unported action became a multi-second trip to JUCE and back, and the rows had to be maintained.

## Watch out
- Where the JUCE app reaches into the engine in-process, the shell needs a control request that mirrors that call; add the request with a test, do not change engine behavior.
- Everything must work without touch (knobs, dial, buttons), including text entry.

In the code: `ui/` (the shell; `ui/PERFORMANCE-APP.md` is the checklist once written), `hosts/standalone/PlayView.cpp`, `PartsView.cpp`, `PartsModel.cpp`, `ParameterGrid.cpp` (the JUCE app being replaced), the keeper in `devices/roland-fantom0/bundle/oscairn-fantom-start` (handover).
