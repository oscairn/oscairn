---
title: "The desktop app starts the Rust UI as a child, the device handover reused"
author: Jacob
date: 2026-11-25
branch: rust-default
cost: { minutes: 3240, tokens: 17270989 }
id: l-c1325a78
scope: repo
status: active
src: agent
touches:
  - hosts/standalone/Main.cpp
  - os/scripts/engine-args.py
  - devices/roland-fantom0/bundle/oscairn-fantom-start
---

Decided (owner, 2026-11-25: "make rust the default now"): a screen without "ui" means rust (engine-args.py, oscairn-fantom-start); OSCAIRN_UI=juce forces JUCE. On a computer the JUCE app stays the process that owns the engine, the MIDI ports and the menu bar; it spawns oscairn-ui from beside itself (host::spawnChild) with --socket and --handover and no profile, hides its window, and on status 3 opens the named app in its window (its Home starts the Rust UI again), on 0 quits. Ruled out for now: a Rust-only desktop app (the engine start, Controllers, menus, file dialogs and plugin editor windows all live in Main.cpp). Watch out: not every start takes this path: a panel profile, --app, --view, --snapshot, --in-process and OSCAIRN_UI=juce keep the JUCE window, so desktop-launch (--snapshot) and window-order (--app) are unaffected; desktop_xshm_segments sets OSCAIRN_UI=juce. Only the FANTOM bundle reads OSCAIRN_UI; the other images' oscairn-session starts JUCE under X.
