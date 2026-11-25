---
title: Panel lamps go over the BMC link; DEC and INC have no known lamp number
author: Jacob
date: 2026-11-25
branch: panel-lamps
cost: { minutes: 3124, tokens: 17022833 }
id: l-06edf3c7
scope: repo
status: active
src: agent
touches:
  - "devices/roland-fantom0/src/BmcLink.h#bmcLamp"
  - devices/roland-fantom0/panels/fantom-07.json
---

Lamps are kind-6 frames to 0x75 written through BmcLinkReader::send (the engine's own ttyGS0 fd, under write_), never through /dev/i2c-slave-2, whose write corrupts its receive queue. The panel map's lamp numbers come from the test tool's table by name agreement; DEC and INC are not in that table and Roland's start trace never writes them, so their entries carry no lamp until identified on the unit (candidates 55 and 57, the table's only gaps). panel.lamps answers a name without a lamp in unknown, quietly, so the UI can ask for dec and inc before the numbers are known. Zone buttons, TEMPO and pads have two or three lamps each and were left out of the map on purpose. BmcLinkReader now opens the device read-write always, since lamps need it even without localOff.
