---
title: "The FANTOM-07 places plugins by chain; the engine's default stays a worker per plugin elsewhere"
author: Jacob
date: 2026-11-17
branch: unit-findings-3
cost: { minutes: 1009, tokens: 1609360 }
id: l-01f6b386
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
- hosts/standalone/profiles/fantom-07.json sets compute.workers "chain". It reaches the engine through os/scripts/engine-args.py (etc/default/oscairn-engine) and through Main.cpp's setenv. The owner decided on the unit's lost blocks in a pedalled passage in Sick tines: a worker per plugin lost 26 to 31, chain 0 to 1, a pool of 3 hosts was unstable.
- The built-in default (plugin_host.cpp's policy(), OSCAIRN_WORKERS unset) stays plugin for desktops and the other profiles. The choice was measured on four A53 cores. A many-core desktop may render better with a worker per plugin, and nothing measured there says otherwise.
## Watch out
- A crash under chain takes down a part's whole insert chain. plugin-groups' killOneChain checks every insert of it comes back with its own gain and the part as loud as before, while the other parts never go silent.
