---
title: macOS release ships oscairn-ui; the child is an accessory app
author: Jacob
date: 2026-11-28
branch: macos-rust-ui
cost: { minutes: 5056, tokens: 25139532 }
id: l-e97f59b9
scope: repo
status: active
src: agent
touches:
  - ui/src/main.rs
  - hosts/standalone/Main.cpp
  - tools/release-macos.sh
---

Decided (unattended, #366): the macOS release builds oscairn-ui (-DOSCAIRN_UI=ON, copied to Contents/MacOS where startRustUi looks; no lookup change). With --handover the child sets winit's Accessory activation policy (slint unstable-winit-030, macOS only) so the Dock shows Oscairn once; it exits when getppid changes (macOS has no parent-death signal). Test: OSCAIRN_UI_PROGRAM names a stub in place of oscairn-ui and permits startRustUi under --snapshot, so the child start, handover (3) and quit (0) run with no window (desktop_rust_ui.py). Unverified, needs a window: Dock icon, menu bar and keyboard focus with the accessory policy, native scaling. gate.sh watch-out: it lists skipped ctest tests (desktop-window-order, desktop-phase3b-engines) as failures and hit a syntax error at its line 22-23.
