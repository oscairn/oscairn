---
title: "EVK and QEMU images run the Rust UI on KMS, no handover"
author: Jacob
date: 2026-11-28
branch: images-rust-ui
cost: { minutes: 5041, tokens: 25044866 }
id: l-88889b0d
scope: repo
status: active
src: agent
touches:
  - os/buildroot/board/common/session-juce-x11/usr/libexec/oscairn-session
  - ui/CMakeLists.txt
  - os/scripts/boot-test.py
---

Decided (unattended, #367): the session script in board/common/session-juce-x11 (name kept: the FANTOM bundle, the rt image and experiments reference it; renaming is its own change) now starts the engine (kept by a subshell, as the old headless branch did) and oscairn-ui under Slint's KMS backend (SLINT_BACKEND=linuxkms-software, rotation from the profile), no X, no --handover (a JUCE screen's tile says it is not here; X and the JUCE app stay built in, unused). ui/CMakeLists.txt chose the desktop (winit) feature for every build but the FANTOM's, so a cross build for an image now gets kms (CMAKE_CROSSCOMPILING). The boot test passes on the QEMU image: Rust UI alone (no Oscairn, no Xorg), 'shell first frame' in /var/log/oscairn-ui.log, screenshot shows Home. Watch out: this needed the target-cpu fix in oscairn.mk (the engine could not load plugins on the QEMU image before).
