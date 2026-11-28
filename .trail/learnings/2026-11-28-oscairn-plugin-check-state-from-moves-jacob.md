---
title: "oscairn-plugin-check --state-from moves each state with the engine's versions.move"
author: Jacob
date: 2026-11-28
branch: moves-183
cost: { minutes: 4745, tokens: 24556644 }
id: l-bb09dac6
scope: repo
status: active
src: agent
touches:
  - "core/control/tests/plugin_check.cpp#stateFrom"
  - core/clap-host/tests/Fixture.cpp
---

Decided: the machine check of a publisher's claim (RFC 0043 step 5) does not rewrite documents itself: it installs both builds unsigned as plugin packages, plays and saves each test state with the old build in a Session of its own, moves the item with Session::moveVersions(..., withoutClaim=true) (one item, so no claim is needed), and compares the moved item's render with the old one's. So it checks exactly the path a device takes, and the move's own refusals (state does not open, ports differ) and its parameter changes are the check's failures. This is why #179's branch comes before #183's. Watch out: PASS_REGULAR_EXPRESSION tests ignore the exit code, which is how the failing cases (v2_changed, v2_sound, v2_ports) are tests that pass; fixture v2_sound plays 10% quieter (-20 dB difference) with every setting the same. Not built: --states <archive> (a library's Sounds as states) and anything in oscairn/packages.
