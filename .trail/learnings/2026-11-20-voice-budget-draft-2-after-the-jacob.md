---
title: "voice-budget draft 2 after the SDK review: IDs, listed slots, units, ages"
author: Jacob
date: 2026-11-20
branch: voice-importance-2
cost: { minutes: 29, tokens: 1737 }
id: l-d8fb5b2c
scope: repo
status: active
src: agent
touches:
  - docs/rfcs/0046-voice-importance-across-parts.md
  - core/engine/tests/vectors/engine-core.vec
  - docs/developer-guide.md
  - core/clap-ext/include/oscairn-clap/voice-budget-plugin.hpp
  - core/clap-ext/include/oscairn-clap/voice-budget.h
  - core/clap-host/rust/src/lib.rs
  - core/control/include/oscairn/control.h
  - core/control/src/json_rpc.cpp
  - core/control/src/remote_control.cpp
  - core/control/src/session.cpp
---

Decided: OSCAIRN_EXT_VOICE_BUDGET stays draft 1's ID and draft 2 is OSCAIRN_EXT_VOICE_BUDGET_DRAFT_2 (the SDK owner's call). This is the opposite of ui.h and assets.h, where the unsuffixed name moves to the newest draft. Both hosts read importance only from a table that answered draft 2's ID and treat a null one as no report, never refusing the plugin. 'Gives up exactly what it listed' needs the listed slots kept (Importance::next), not a shared key: between the list after block k and the drop at k+1's start, ages, levels and the 50 ms mark move, and victim() worked out again picks another voice. Candidate.age must be a note-on serial so a reused slot is skipped. Units, not weight, are counted (voices, limits, n); weight is only the host's cost estimate. A VA whose limit counts engine voices lists engine voices at weight 1. Watch out: voice-importance-scenario under TSAN fails its 'piano loses nothing for the lead's own lane' claim intermittently at feaa2011 too (4 of 6 alone there, 1 of 4 on this branch), and it fails more often with another heavy job on the machine. The SDK repo's bindgen output equals the host's vendored oscairn-clap-sys byte for byte; the core's own bindgen output differs from the checked-in files in rustfmt layout and bindgen's _bindgen_ty numbering, so carry only MADE_FROM and the numbering over by hand.
