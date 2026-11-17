---
title: "Panel holder: replacing an old holder without closing the device"
author: Jacob
date: 2026-11-17
branch: panel-holder
cost: { minutes: 1330, tokens: 2223784 }
id: l-bc327ef9
scope: repo
status: active
src: agent
touches:
  - devices/roland-fantom0/src/panel_reader.cpp
  - devices/roland-fantom0/bundle/oscairn-fantom-start
---

decided: the new holder asks the running one over its own socket (OSCAIRN-PANEL-HANDOVER <version>) and gets the device, the listener and every reader by SCM_RIGHTS, so readers never reconnect and the open file never loses its last reference; start never stops a holder, an old one that cannot answer is kept with one logged line until power-on. ruled out: killing the old holder (a closed device silences the panel for good); a version read from /proc (the binary answers for itself: exit 3 refused, 4 silent). watch out: a SIGIO between the last look and the blocking ioctl is lost (the test caught it on macOS about half the time), hence the flag re-check and a 1 s timer; macOS pty master reports a hang-up only to poll(POLLIN), and tcgetattr on the master fails there, so the seam sets raw mode on the slave it opens; the old holder only counted a reader dead when a send failed, so reap must run before the limit check on every accept.
