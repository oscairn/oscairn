---
title: "A Performance's estimate counted each part of a sharing sampler as a whole instance"
author: Jacob
date: 2026-11-16
branch: multi-piano
cost: { minutes: 459, tokens: 560090 }
id: l-05c47b27
scope: repo
status: active
src: agent
touches:
  - "core/control/src/budget.cpp#Session::estimate"
---

Question was whether parts on one sample pack count the pack twice. Found: the Sampler never maps its packs (pread, one open per process, decoded heads cached per process), so there is no shared mapping to count once and VmLck holds no samples; what VmLck double counts across workers is shared code. The real over-count was Session::estimate(directory), the preflight of a Performance not open: it priced every node at its plugin's measured instance memory, while the load puts parts of a sound-contexts plugin in one instance (up to its capacity), so five sampler parts were priced as five instances. Fixed by recording a build's carrying capacity (measurements.json contexts) and pricing an instance's first node in full and the others at the declared total; saved sharing and OSCAIRN_SHARING=cpu are handled (private copies still count each). A measurement of memory is only recorded from an instance playing one sound, so the full price needs the plugin to have played alone once. Unverified on the FANTOM: that its Carol Bells load puts the five sampler parts in one worker (14 workers fits one sampler, one synth, twelve effects).
