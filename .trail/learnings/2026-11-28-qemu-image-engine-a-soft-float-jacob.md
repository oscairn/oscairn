---
title: "QEMU image engine: a soft-float Rust host from a big.LITTLE target-cpu"
author: Jacob
date: 2026-11-28
branch: buildroot-rust-cpu
cost: { minutes: 4576, tokens: 24323204 }
id: l-1f3f03db
scope: repo
status: active
src: agent
touches:
  - os/buildroot/package/oscairn/oscairn.mk
---

Cause found (unattended, #367): oscairn.mk passed -C target-cpu=$(BR2_GCC_TARGET_CPU), which is cortex-a76.cortex-a55 for the QEMU image; rustc does not know the pair and compiles for a CPU with no floating point, so rust_clap_new takes its double in x2 while the C++ caller puts it in d0. The plugin worker loaded plugins at a garbage rate and frames 0: 'Cannot load org.oscairn.step-sequencer: Invalid sample rate or maximum block size', the engine exited, and the JUCE app on the same image failed the same way. Seen by printing the arguments at each call and by objdump of rust_clap_new (str x2 for the rate). Fix: the first name of the pair. Watch out: any new big.LITTLE defconfig gets the same treatment; a C-ABI function with a float argument is where a wrong target-cpu shows.
