---
title: Rust UI slow first frame after FANTOM boot
author: Jacob
date: 2026-11-25
branch: ui-cold-start
cost: { minutes: 27, tokens: 79723 }
id: l-eb648836
scope: repo
status: active
src: agent
touches:
  - os/buildroot/external.mk
---

On the FANTOM (Linux 4.19, no credited hwrng) the kernel's crng is ready only about 66-85 s after boot, sooner if network traffic arrives (an SSH connection stirs it, and dropbear itself waits for it). Anything calling getrandom with flags 0 blocks until then: glibc 2.36+'s arc4random and getentropy do. Expat salted its hash with arc4random_buf, so fontconfig's first parse held oscairn-ui before Slint's backend was made (state S, wchan wait_for_random_bytes, /proc/PID/syscall 278 with 16 bytes and flags 0). Fixed by building Expat --without-arc4random-buf --without-arc4random --without-getentropy (external.mk): first frame 1.3 s after the UI's start. Not a CPU starvation and not the font cache (rebuilt in tmpfs each boot: 0.7 s). A first-frame watch from process start cannot cure such a stall: each restart blocks again, and the keeper gives up after four exits in a minute. To check a new boot-time stall, look for crng init done in dmesg against the first frame; the ms= timing lines are wall clock and jump when go.sh sets 2022-01-01 and later 2026.
