---
title: "FM operator view: horizontal flow, operator colors, dots"
author: Jacob
date: 2026-11-16
branch: rust-app-fm
cost: { minutes: 213, tokens: 422463 }
id: l-abda9c7b
scope: repo
status: active
src: agent
touches:
  - docs/rust-ui.md
  - ui/PERFORMANCE-APP.md
  - ui/PROGRESS.md
  - ui/slint/grid.slint
  - ui/slint/play.slint
  - ui/slint/shell.slint
  - ui/src/grid.rs
  - ui/src/knobs.rs
  - ui/src/main.rs
  - ui/src/play.rs
---

The FM view (ui/src/fm.rs) is the owner's design, not the reference's (ui/GRID.md records it). Live dots along the envelope lines need audio-thread state that no layout data carries; the plugins team decided (2026-11-15) it will not go into oscairn.ui draft 6: a separate optional extension (a generic positions table, one function the host calls on the main thread, a way to say nobody is watching). The host side is built and tested (Screen::set_positions, nothing runs at rest); the plugin side is only a proposal outside the repo. Operator colors are derived from the theme's accent by hue rotation in OKLCH (the wire colors are not used: one is a gray and themes repeat them), lightness limited to 0.3-0.86 because at the ends of the range hues stop differing (the odd theme's yellow accent made all eight near white before). Tests measuring lines in rendered pixels find the panel's origin to a whole pixel: do not measure at a fractional scale (1.55 gave 0.5 to 1.5 px false offsets for dots).
