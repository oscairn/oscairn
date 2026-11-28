---
title: "parts.useVersion compares parameters between two offline instances, not with the playing one"
author: Jacob
date: 2026-11-28
branch: moves-179-open
cost: { minutes: 4955, tokens: 24968025 }
id: l-9ba3168f
scope: repo
status: active
src: agent
touches:
  - "core/control/src/versions.cpp#Session::useVersion"
  - "core/control/src/session.cpp#Session::sessionStatus"
---

Decided: parts.useVersion (RFC 0043 step 3) saves each node's state, opens it in an offline instance of the playing build and one of the new build, compares their parameters, and only then replaces the nodes (Session::replace plus loadSnapshot) and calls record() once, so edit.undo brings the old instances back as one step. Ruled out: comparing with the playing host's own values: its cache holds what it was last told, so the fixture's read-only parameter 99 read 0.5 there while the plugin said 0.8, a false change on every move. Watch out: session.status gained mixedVersions, and core/control/tests/json_rpc.py compares the whole status object, so every new status field must be added there; nodes sharing an instance are refused by useVersion, not moved.
