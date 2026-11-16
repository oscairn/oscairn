---
title: "keyboard-window failed about 1 run in 10 because the test's always-on-top was undone by the window's own timer"
author: Jacob
date: 2026-11-16
branch: flaky-two
cost: { minutes: 21, tokens: 24829 }
id: l-4626e611
scope: repo
status: active
src: agent
touches:
  - hosts/standalone/tests/keyboard_window.cpp
  - hosts/standalone/KeyboardWindow.cpp
---

## Decided
The test no longer sets the keyboard window always on top. It posts the click to the keyboard component directly, which needs no window order, and the window's own 250 ms timer (KeyboardWindow, timerCallback) sets always-on-top back to whether the process is the foreground one, which a test's never is (tests/hidden_windows.mm keeps it from activating).

Why: a debug trace of a failing run showed `test setAlwaysOnTop(true)` at t, `test mouseDown` at t+244 ms, the timer's `setAlwaysOnTop(0)` at t+267 ms, and at once `activeWindowStatusChanged active=0`, which lets go of every held key. So the engine had no key held and the key showed up. The failure depended on where the click fell in the timer's 250 ms phase. Alone: 2 failures in 21 runs before, 0 in 40 after.

## Tried and ruled out
- Another app's window taking the front, the velocity at the click's y, a repaint wait: the one traced failure shows no sign of them; every status change in it was to "not active".

## Watch out
- Any test that changes a keyboard window's level or active state while holding a key races that timer; letting go on a level change is the product's wanted behavior.

In the code: hosts/standalone/KeyboardWindow.cpp (activeWindowStatusChanged, timerCallback); hosts/standalone/tests/keyboard_window.cpp.
