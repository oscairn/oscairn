---
title: "A FANTOM cold start needs Roland's start words for its panel and sound"
author: Jacob
date: 2026-11-25
branch: cold-dsp
cost: { minutes: 3288, tokens: 17368799 }
id: l-c0709627
scope: repo
status: active
src: agent
touches:
  - "devices/roland-fantom0/src/BmcLink.h#bmcLinuxUp"
  - "devices/roland-fantom0/src/BmcLink.h#bmcPanelStart"
  - "devices/roland-fantom0/src/BmcLink.cpp#BmcLinkReader::open"
  - devices/roland-fantom0/bundle/oscairn-fantom-start
  - "devices/roland-fantom0/src/DeviceReaders.cpp#openKeyboard"
---

A cold start (Roland's mi078 not run since power-on) stayed silent even with the SAI receive side set by oscairn-fantom-start's arecord (step 5b). On the unit, 2026-11-25, with the owner listening: kind-9 selector 3 to 0xE1 alone was still silent; selector 2 (12 00 e1 02 01 00 00 00 6c 00 c1 04, 'the Linux side is up', starts the BMC's 112-byte status frames 10/s) then selector 3 (12 00 e1 03 ff ff 00 00 e8 3c e9 00) gave Oscairn's sound. Both the arecord and the two frames are needed, so BmcLinkReader::open sends bmcLinuxUp then bmcSelector3 after Local Switch off, each one write, only when OSCAIRN_BMC_COLD is set (oscairn-fantom-start --cold sets it; a warm start after mi078 sends neither, since resending there is untested). Selector 2's payload is not a constant: mi078 (0x565c30) freads the 256-byte /home/ROLAND/FANTOM/ADJUST/KY019.ADJ, a per-unit file carrying a serial, and sends the int32 at offset 16, or 2 when the file is missing, short or the word negative (0xcf9298's csel); the owner's unit has 1. The engine runs in the bundle's chroot and cannot see /home/ROLAND, so the start script reads the word with od and passes it. Bytes past each frame's count (byte 11) are stale mi078 memory, kept verbatim; the BMC's selector-2 handler is not seen to read even the payload, and no selector-3 handler was found, so which frame does what for audio is not known. The status frames were already dropped quietly (kind 9 long from 0xE1 in BmcLinkReader::frame), with the parser's fixed buffer. The same day the panel (knobs, buttons, dial, sustain pedal) proved dead on a cold start until mi078's first write, the panel start word 0c 00 75 00 80 30 00 00 01 00 00 04 (kind 6 to 0x75), was sent; after it the panel worked. So a cold open sends bmcPanelStart first, then Local Switch off, then selectors 2 and 3, as mi078 orders them.
