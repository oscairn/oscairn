---
title: Session setup lives in the engine; the UI sends only its own memory
author: Jacob
date: 2026-11-14
branch: rust-ui-setup
cost: { minutes: 78, tokens: 115233 }
id: l-08da2a85
scope: repo
status: active
src: agent
touches:
  - hosts/standalone/SessionSetup.cpp
  - hosts/audio/AudioEngine.cpp
  - hosts/standalone/Main.cpp
  - ui/src/shell.rs
  - core/control/src/budget.cpp
---

Decided: the budget, memory classes, always-available apps, device repositories and capture length are engine policy, applied by the engine itself at start when it is given the device profile (--device-profile, which engine-args.py passes for a profile with a panel), through setUpSession in hosts/standalone/SessionSetup.cpp. The JUCE app calls the same function for the engine it starts, which is never given the profile; so neither UI re-implements it. Why: the Rust shell only ever sent performance.reopen, and on the keyboard the Rust path left cpu 0, no memory classes and no capture (measured on a unit). Ruled out: copying Main.cpp's arithmetic into the Rust shell (two places to keep equal), and passing the profile to the JUCE app's own engine (it would also take the profile's control groups before the app sends its own, a JUCE-path change). The one part that is a UI's own is its resident memory, which used to be folded into budget.set's system: it is now memory.shell, sent by each UI on connecting and replacing the last, so the JUCE app and the Rust shell taking turns at the display never count twice; reservedMemory adds it to system, so every figure stays as before. Watch out: an engine started without --device-profile applies nothing; the JUCE app with --snapshot skips record.retro (the comparison test sends it itself).
