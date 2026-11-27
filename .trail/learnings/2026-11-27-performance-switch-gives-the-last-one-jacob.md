---
title: "Performance switch gives the last one up in steps: same-state keep, silent first, least missed faded, cut last (issue 330)"
author: Jacob
date: 2026-11-27
branch: switch-steps
cost: { minutes: 4206, tokens: 21556894 }
id: l-165a4bc7
scope: repo
status: active
src: agent
touches:
  - "core/control/src/performance.cpp#Session::openPerformance"
  - "core/control/src/performance.cpp#Session::holding"
  - "core/control/src/performance.cpp#Session::fadeOut"
  - "core/control/src/budget.cpp#Session::estimate"
  - "core/control/src/performance.cpp#Session::read"
  - core/control/tests/performance_switch.cpp
  - docs/control-protocol.md
---

decided: the owner's rule (2026-11-27): the new Performance loads; held notes ring only as far as that allows. (A) An incoming node keeps an open instance as it is, sounding or not, when that instance opened from the node's saved state (loaded_ source with the file's stamp, or stateOf_ digest equal) and nothing changed it since: Session::unsaved() false after record(), and no plugin-reported parameter value differing from observed_ (edited_, set in Session::service). No save is made to compare: a whole-state save of a sounding worker instance is a control silence and a reset (learning 2026-11-21). The kept node is in Prepared::taken (not carried over) and Prepared::held, whose notes open() moves with Graph::takeNotes; the estimate leaves kept nodes out of the outgoing side, so TSO -> TSO with a chord needs one copy. Session::holding is called in document order by both read() (holds, before claim and take, which skip holders) and estimate(), so they agree; both then drop (Session::unfed) a kept node that an outgoing node not kept plays into while it sounds: a beside tail rings out through its own chain carried over, and Graph::ringOut leaves out a route into a node no longer retiring (control-shared-parts' dressed reload caught it). (B) Session::estimate with Steps (only from openPerformance): silent nodes first (busyNodes, the freeInstances rule), then sounding 'sources' (instruments, or nodes no sounding node feeds) least missed first: leaving_ tails, then Graph::sounding voices, then most memory; each given up with what it alone keeps busy; a set leaving nothing to ring is a cut (steps.cut unless e.fades). budget.estimate's own seamless still counts all but kept nodes. (C) Sounding releases and the cut fade before read() via Session::fadeOut: Graph::fadeFrom puts releaseSlot (254) on routes out of the released set, the control thread steps it 1->0 over 150 ms (sleeping), so instances are then taken over as a cut's (no extra memory) and the reset is inaudible; a failed load unfades. ruled out: a post-switch fade for the memory case (the faded instances could not be taken over, so the incoming loads fresh beside them: memory both, the thing the rule prevents); tracking 'unchanged' by valueRevision (moves on settle and on claimed sounds' selection) or by comparing all values (a claimed instance's values change after the cue). watch out: the fixture instrument reports param 7 every block, so edited_ only counts a value that differs. The CPU-only whole fade (fades) is reached only when nothing fits beside; with three audio cores the incoming chain sets the block budget and some outgoing instruments always fit, so the old fade test runs at OSCAIRN_AUDIO_CORES=1. Tests that set a cut budget of open+next-1 with a note held now get a partial release; use next alone for a cut. An audition's both-must-fit check uses the estimate, which leaves kept nodes out; auditioning the open Performance itself is the one case that undercounts (keepInstances_ keeps nothing). The load blocks 150 ms more when it fades.
