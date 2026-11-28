---
title: "a plugin that refuses its state no longer refuses the Performance: it opens as a silent stand-in (issue 331)"
author: Jacob
date: 2026-11-28
branch: open-with-failed-part
cost: { minutes: 13, tokens: 118198 }
id: l-7e93d2b4
scope: repo
status: active
src: agent
touches:
  - core/control/src/performance.cpp
  - core/control/tests/refused_state.cpp
---

Decided: Session::read's per-node restore goes through restoreOrStandIn (performance.cpp): a throw from restore that says "CLAP state restore failed" (stateRefusal: the plugin was given its state and refused it; a missing or damaged state file and changed ports stay refusals, as control-performance, control-performance-format and control-not-blocked-failing expect) swaps the loaded node for standIn(..., refused=true), which already keeps the saved snapshot byte for byte and writes it back on save, and adds one notice. Silent, not the plugin's init patch: a default sound playing would hide the fault, and a stand-in already does this for a plugin that is not installed. Not for a memory refusal, and not when workerFailures() rose during the restore: readTaking reads again for a dying worker (2026-11-17 learning), and swallowing that into a stand-in would turn a retry into a failed part. Covered: fresh loads and warm free instances (restoreTaken called inline). Not covered: a playing instance taken over at the cue (the later list, restoreTaken after wires exist): its stand-in swap would leave wires, parts and names holding the old runtime id; if its fresh fallback also refuses, the Performance is still refused. Shared-engine paths (savedSound, alone) unchanged. The UI needed only a bar notice (shell.rs say_new_failures); the Parts list, Mixer and Chain already mark failed nodes from session.status failures. The old refused_state test asserted the opposite rule (an open refused naming the plugin); it now asserts the open, failures, notice, byte-for-byte save and reload.
