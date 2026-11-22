---
title: "Removing a package: what the device can and cannot say uses it"
author: Jacob
date: 2026-11-22
branch: package-remove
cost: { minutes: 120, tokens: 118198 }
id: l-790ee39c
scope: repo
status: active
src: agent
touches:
  - core/control/src/performance.cpp
  - core/control/tests/package_removal.py
---

packages.removal (Session::packageRemoval in core/control/src/performance.cpp) reckons who uses a package from three records only: packages.json needs, each stored plugin's assets.tsv, and each library item's document (plugins by format, ID and version; asset references by library or content hash). Two limits found while building it. First, an item maps to a plugin package only when the package's declared plugin version equals the version the CLAP itself reports, because the Performance document records the descriptor's version: the test fixture reports version 1, so a package built as 1.0.0 was never seen as used (core/control/tests/package_removal.py packs it as 1). Second, which items use a library's samples or waveforms cannot be told per package: they are appended by content to the device's own packs (assets/samples/pack-*.osp) for every library alike, and library.uninstall leaves them there, so a library's dependents are only the packages whose dependencies name it. Plugin builds a library archive carried are recorded in that library's record but stay in the store when it goes, so packageRemoval takes plugins only from records of kind plugin. Removal under the open Performance or a warm instance is refused even with evenIfUsed; the warm refusal has no test.
