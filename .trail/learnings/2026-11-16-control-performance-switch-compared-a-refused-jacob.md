---
title: control-performance-switch compared a refused load with its budget still on
author: Jacob
date: 2026-11-16
branch: tsan-three
cost: { minutes: 655, tokens: 994930 }
id: l-89d0894c
scope: repo
status: active
src: agent
touches:
  - core/control/tests/performance_switch.cpp
  - core/control/src/budget.cpp
  - core/engine/rust/src/notes.rs
---

## Decided
The refused-load case sets s.setBudget({}) before compare(), as every other comparison in the test does.

Why: a budget with cpu above 0 keeps the load guard engaged whatever OSCAIRN_LOAD_GUARD says (Session::measure sets the voice budget to cpu times the period; the guard-off setting only applies without one). The case left the cut budget on while it rendered, so a block descheduled on a loaded machine gave a note up in the switched Session and not in the fresh one: 'different at block 8 (guard 1)', then the held notes made the old instances never ring out (leaving stays true) and engine.timing list 8 instances. Reproduced in Docker with GCC ThreadSanitizer, 2 CPUs, busy loops: the whole test 1 of 24 runs (0 of 24 at 0a684842 under 4 busy loops); the test stopped after that comparison: 2 of 36 failed, 0 of 36 with the line added; the whole test after: 0 of 30.

## Tried and ruled out
- Graph::collect (3606d4ac): lets a host go only once nothing but its list holds it; no timing in it.
- A memory-pressure event: none seen in the failing runs; the one 'memory short' line is the test's own memoryPressure() call (line 527), and passing runs print it too.

## Watch out
- The other three failures in the CI run were separate causes; a failure at this comparison also fails the checks after it.

In the code: core/control/tests/performance_switch.cpp (the refusing copy); core/control/src/budget.cpp measure().
