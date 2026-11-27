---
title: "A plugin update borrows the older version's measured figures for its estimate"
author: Jacob
date: 2026-11-27
branch: estimate-versions
cost: { minutes: 3731, tokens: 20122117 }
id: l-d836ecd2
scope: repo
status: active
src: agent
touches:
  - "core/control/src/budget.cpp#Session::measurementFor"
---

Decided: measurementFor, with no record for id@version, merges the newest OLDER version's records (assets::older order) and marks the Measurement/Cost 'earlier'; budget.estimate then shows measured false, earlier true while Cost.measured stays true inside, so estimateOf's quartering and guessed-plugin rules treat the figures as measured, not guessed (marking them unmeasured would have made a borrowed figure never refuse and never count). The states link to an older version's key is followed only while the new version has no record of its own (measurementFor(measuredKey)->earlier), so its own record wins at once. An older version never borrows from a newer one: nothing found, the package estimate or default as before. Why: a plugin update had made the engine forget its sounds' figures, so estimates and the ring-out fit rule were wrong until replayed. Watch out: a key with a declaration (#N) borrows the older version's merge over all declarations, not the same one; the saved-state link path is not covered by a test (stateDigest is private), only the version fallback in control-estimates; the FANTOM check (Sampler update, budget.estimate before any load) is still to be done.
