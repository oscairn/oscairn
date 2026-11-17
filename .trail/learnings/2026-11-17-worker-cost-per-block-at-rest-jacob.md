---
title: Worker cost per block at rest rose on the unit but not on the Mac; the output guard over every channel is the first suspect
author: Jacob
date: 2026-11-17
branch: unit-findings-3
cost: { minutes: 30, tokens: 73711 }
id: l-88a37719
scope: repo
status: active
src: agent
touches:
  - core/control/include/oscairn/busiest_load.h
  - core/control/include/oscairn/control.h
  - core/control/include/oscairn/session.h
  - core/control/src/budget.cpp
  - core/control/src/performance.cpp
  - core/control/tests/admission.cpp
  - docs/control-protocol.md
  - docs/rfcs/0025-resource-budgets.md
  - core/control/tests/value_reports.cpp
  - core/engine/include/oscairn/sound_context.h
---

## Decided
- Not fixed: the cause is not proven. Mac bench, alternating base 87a49934 (e16f7e4d + memory) and 02f72824, 5 runs each, worker audio thread CPU per block (proc_pidinfo thread times): ST Final's shape with cheap fixture plugins was 11.95-15.02 us against 12.00-14.98. Five parts sharing a basic synth (9 stereo ports) gave 35.4-40.7 against 35.3-40.2. No difference, so no bisect.
- On the unit only instruments rose (Bass Sampler 41 -> 58-61, Arp 40 -> 56-60, Sick tines' fm2 64 -> 80). Two-channel insert effects stayed at about 36.
## Watch out
- Candidates, in order:
  1. Worker.cpp's guard(n), added with plugin groups. It runs every block for every node and scans every output channel (many on a sound-context instrument, inactive buses included, possibly cold in cache). Its result is needed only when another node of the same worker mixes from that node. Under plugin or chain placement, an instrument alone in its worker never needs it.
  2. The shared block header grew by 64 fused routes (about 2.3 KB) before the audio planes, which moves their offsets and alignment.
  3. Anything else on the worker's audio thread (perf record will show it).
- To tell them apart on the keyboard: perf record -t <the Bass worker's audio thread> for 10 s at rest in ST Final on both bundles and compare the guard share; or try a build that skips guard for nodes no route reads.
- The unit's "us/block" (m22.py "aud") is the audio thread's whole run time from /proc over blocks, not engine.timing's cpu figure.
