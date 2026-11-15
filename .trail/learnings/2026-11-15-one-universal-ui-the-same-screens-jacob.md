---
title: "One universal UI: the same screens serve the keyboard shell, resizable desktop windows and later a DAW plugin, fully themed"
author: Jacob
date: 2026-11-15
branch: main
cost: { minutes: 57, tokens: 179302 }
id: l-83d9172c
scope: repo
status: active
src: agent
touches:
  - ui/src/theme.rs
  - ui/slint/theme.slint
  - ui/src/main.rs
  - hosts/standalone/Theme.cpp
---

## Decided
- The owner wants one UI code base with three homes: the keyboard's single full-screen shell, desktop windows (separate windows where the JUCE app has them, resizable, scaled by the OS on Retina), and later a plugin editor inside a DAW. So a screen is a component that knows nothing about its window, reflows for any size (smaller and larger device screens too), forces no scale on desktop, and keeps no process-wide state, because a DAW may open several instances and owns the window and event loop.
- The theme system is part of that: colors, button styles (pill included), corner rounding, spacing and type sizes all come from the theme, for every widget including the parameter grid; nothing is hardcoded.
- The parameter grid (tiles with knobs, sliders, switches, dropdowns, split buttons, graphs, laid out from the plugin's own data) is the main way a plugin is edited and must exist in any port; a list of rows is not a substitute.
- On a hardware device the text keyboard overlays about the lower half of the screen and leaves the edited field visible.

## Tried and ruled out
- Building the Rust shell for the keyboard only (fixed 1280x720, forced scale, one shell with stacked screens, parameter pages as rows): it had to be reworked once the desktop and grid requirements were stated. A real-window run on macOS showed no separate windows, no minimum size, fixed 11-row lists that clip on resize, no mouse wheel.

## Watch out
- A device profile's scale must be forced only on a device build; with it set on desktop the OS scale is ignored.
- Known debts are listed in the shell's grid notes: a single thread-local app, shared Slint globals, the shell owning the client and timers.

In the code: `ui/src/grid.rs`, `ui/slint/grid.slint`, `ui/src/theme.rs`, `ui/slint/theme.slint`, `ui/src/main.rs` (SLINT_SCALE_FACTOR), `hosts/standalone/Theme.cpp`, `ParameterGrid.cpp`, `TextKeyboard.cpp` (the JUCE references); branch `rust-grid`.
