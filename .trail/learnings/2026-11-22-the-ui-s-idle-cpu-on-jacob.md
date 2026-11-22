---
title: "The UI's idle CPU on a device: three dumb buffers made every frame a whole repaint"
author: Jacob
date: 2026-11-22
branch: ui-idle
cost: { minutes: 29, tokens: 484410 }
id: l-9da19909
scope: repo
status: active
src: agent
touches:
  - ui/vendor/i-slint-backend-linuxkms-1.18.1/display/swdisplay/dumbbuffer.rs
  - ui/vendor/i-slint-backend-linuxkms-1.18.1/display/swdisplay/age.rs
  - ui/vendor/i-slint-backend-linuxkms-1.18.1/renderer/sw.rs
---

Decided: oscairn-ui's idle cost on the FANTOM-07 (1.5 to 3.75% of a core) was the frames the top bar's meters ask for, each painted whole. Slint's linuxkms DumbBufferDisplay rotated three buffers, so the back buffer was always age 3, and renderer/sw.rs maps any age but 1 and 2 to RepaintBufferType::NewBuffer. Measured on the unit (bundle eb3dad6f, 28 s, /proc schedstat and the fb id in /sys/kernel/debug/dri/0/state): 41 flips, 553 ms on the main thread, 13.5 ms a frame; the engine-reader thread 22 ms in 30 s; no other thread. Fix: two buffers (dumbbuffer.rs, age.rs), the back one age 2 = SwappedBuffers, and map_back_buffer waits for the flip before it draws. Ruled out: a shadow buffer in cached memory copied to the dumb buffer (the vendor README: slower frames, audio misses on the unit); keeping three buffers and marking earlier frames' regions dirty (SoftwareRenderer::render returns the union it painted, never one frame's own region, so the marks never shrink); the Mac tests' idle counts, which use ReusedBuffer and cannot see a buffer's age. Watch out: the vendored crate compiles only for Linux (pkg-config for fontconfig stops a cross check on the Mac), so only age.rs and the test tests::the_devices_buffers_repaint_only_the_meters_for_a_meter_change run on the Mac; a frame after a NewBuffer frame is whole too (the renderer clears its cache), so the first four frames are whole.
