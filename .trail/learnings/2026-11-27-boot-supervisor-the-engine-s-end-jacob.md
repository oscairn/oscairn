---
title: "Boot supervisor: the engine's end is Oscairn's end, not the UI's"
author: Jacob
date: 2026-11-27
branch: supervisor-engine
cost: { minutes: 26, tokens: 25040 }
id: l-d219c587
scope: repo
status: active
src: agent
touches:
  - core/clap-host/tests/Fixture.cpp
  - core/control/CMakeLists.txt
  - core/control/tests/refused_state.cpp
  - devices/roland-fantom0/bundle/oscairn-fantom-boot
  - devices/roland-fantom0/bundle/oscairn-fantom-restart
  - devices/roland-fantom0/bundle/oscairn-fantom-start
  - devices/roland-fantom0/tests/fantom_start.sh
  - docs/fantom-0.md
  - os/buildroot/package/oscairn/oscairn.mk
  - os/scripts/check-image-profile.py
---

Cause: watch() counted oscairn-ui as Oscairn running, and keep_ui in oscairn-fantom-start keeps the UI alive after the engine dies, so a killed engine (kill -9, device test w27/sv1 C2) left a frozen screen and silence with no restart or fallback; true before 56c9d359 too. Decided: watch() requires the process named Oscairn or oscairn-engine; restart() first runs oscairn-fantom-stop when an oscairn-ui is left (the restart command's path, which ends the keeper through the stopped mark), then starts through the same counted path; a failed start removes the stopped mark so it counts as another end. Ruled out: restarting on an unanswered socket (a long Performance load is busy, not dead); ending the UI from the start script when the engine ends. Watch out: found by process name, not engine.pid (that pid is the chroot wrapper's, not checked to be the engine's); stray workers are not stopped explicitly (unverified on the unit); a UI alone killed reconnecting to the engine is unverified.
