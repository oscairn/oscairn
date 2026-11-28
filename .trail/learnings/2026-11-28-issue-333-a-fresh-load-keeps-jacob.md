---
title: "Issue 333: a fresh load keeps nothing; sound and scene changes were not seen as changes by the keep"
author: Jacob
date: 2026-11-28
branch: reload-fresh
cost: { minutes: 4950, tokens: 24960473 }
id: l-b5d4b901
scope: repo
status: active
src: agent
touches:
  - "core/control/src/performance.cpp#Session::holding"
  - core/control/include/oscairn/session.h
  - "core/control/src/parts.cpp#Session::startScene"
  - ui/src/library/open.rs
  - core/control/tests/performance_switch.cpp
---

decided: issue 333 said case 1 (edit, then reload to get the saved state back) was already covered by #330's keep. It was not for two kinds of edit: Session::prepareSound/selectSound/discardSound/setProperty go straight to the host and do not make the Performance unsaved, and a scene recall is deliberately not an edit (startScene's Quiet: 'switching scenes is playing'), so after either a pedaled reload kept the instance as it was and the saved sound or values never came back. Now those calls and startScene put the nodes in edited_, which holding already skips (edited_ is cleared when the next Performance is published); a parameter (undo.cpp setParameter) and the mix already made unsaved() true. Case 2: performance.load and library.load take fresh:true (Control::loadPerformance(dir, fresh), Session::fresh_ for the load): holding returns 0, claim/playingEngines/playing() offer nothing, readTaking gets playing=false and no warm instances, a prepared read is not published; every node loads anew and what sounds rings out beside. The owner's 'same Performance twice within a couple of seconds' is in the UI (library/open.rs): the open Performance (performance.info's id) pressed again within 2 s of this screen opening it sends fresh. ruled out: putting that rule in the engine: the device scripts reload the same Performance every 0.5 s to measure (#340's pedaled reloads), so every scripted reload would have been fresh. A rule on presses alone (not on the open Performance) broke the save question's flow, where the same row is pressed again after Cancel. watch out: after a scene change or a sound picked, a pedaled reload of the same Performance no longer keeps its instances, so it needs memory for both copies or the stepwise release (a player may hear a release where they did not before). Not built: a Reload fresh action in a Performance menu and a panel gesture (the Rust UI has no Performance menu yet); the old instances of a fresh load may still be kept warm once rung out and reused by a later ordinary load (taken over with reset and state, as before #330).
