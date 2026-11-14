---
title: Shared instances were never told their sound buses were active
author: Jacob
date: 2026-11-14
branch: shared-ports-active
cost: { minutes: 6, tokens: 2907 }
id: l-4bfa4c3f
scope: repo
status: active
src: agent
touches:
  - "core/control/src/parts.cpp#tellActiveOutputs"
  - core/control/src/session.cpp
  - core/control/tests/shared_parts.cpp
  - plugins/basic-synth/basic_synth.cpp
---

Two features that each worked alone did not meet: parts sharing one instance (RFC 0021, each sound on its own output bus through SoundContext) and clap.audio-ports-activation, where the porting guide tells plugins a port never named active is inactive and its sound goes to main. Session::tellActiveOutputs walked nodes and skipped any with fewer than two output ports, and a SoundContext shows only its one port, so the real instance under a group was never told its buses; a plugin following the guide played every shared sound through the first part, past the joiners' levels and inserts, their meters at zero. Nothing caught it because the older plugins did not implement ports activation and control-shared-parts compared only the master mix, where a sound on the wrong bus at unity level sounds the same. Decided: tellActiveOutputs computes one mask per real instance (keyed by owner of the weak_ptr) from the graph on every compile, main ports plus each SoundContext's bus, including the last Performance's contexts still ringing out (leaving_); a per-context contribution in SoundContext was ruled out because it would follow object lifetimes (contexts outlive their nodes until the plan retires) rather than the graph. The Basic Synth now implements the never-named-is-inactive rule so the tests can see a wrong bus, and the tests assert each part's own meter with levels and an insert. Watch out: a worker restart of a shared instance comes back with the sounds' other parameter values (the instance's last published values are reapplied), and one opened from a saved Performance loses its per-channel selections or buses; both are outside activation and not fixed here. Also enforced on the add path (sharingSource, Session::lead) what formable enforces on load: a node with an audio wire from a non-main port is never grouped.
