---
title: Parts apart by default; a saved engine split as it opens; sharing only where apart does not fit the memory
author: Jacob
date: 2026-11-17
branch: sharing-default
cost: { minutes: 1752, tokens: 3239642 }
id: l-fd512c67
scope: repo
status: active
src: agent
touches:
  - "core/control/src/budget.cpp#Session::sharing"
  - core/control/src/performance.cpp
  - "core/control/src/session.cpp#Session::sharingSource"
---

decided: the sharing decision moved out of SoundContext::sharesOnItsOwn (core/engine, not touched; now unused, its comment stale) into Session::sharesParts: OSCAIRN_SHARING memory or balanced shares, unset or cpu apart. No profile field: the code has one default and every profile gets it. Session::sharing(dir) estimates apart and, when apart does not fit the memory ceiling and shared does, returns the shared estimate with a notice; openPerformance and preparePerformance pass it to readTaking/read, so the estimate that admitted it and the layout read agree, and budget.estimate and the library's fit use it too. Only memory triggers the fallback: sharing never frees CPU. ruled out: keeping saved shared Performances opening shared (RFC's old rule): every Performance a user saved since sharing existed would have stayed shared forever, saved again shared. The split uses save_sound (sound-snapshots draft 4), which busParameter already requires of any plugin that can share, so a split is always possible: the root's instance loads the engine snapshot, each sibling a fresh instance loaded from root->saveSound(sound on its channel), then the root reloads its own sound alone; the per-node values snapshot is not reapplied (the engine snapshot holds the same values; control-shared-parts compares sample for sample: 0 difference). Ports are not checked for split nodes (a saved shared node lists one output port, the plain instance all). Watch out: the root holds every sound of the engine until the siblings have theirs, so a split open needs the engine's samples about twice briefly; not in the estimate. addNodes (a Sound loaded into a part, imports) also joined playing instances whatever the bias; it now asks sharesParts, but a saved engine brought in by an import stays shared.
