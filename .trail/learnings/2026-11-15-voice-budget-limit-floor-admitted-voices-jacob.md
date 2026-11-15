---
title: "Voice-budget limit floor: admitted voices, not an idle estimate"
author: Jacob
date: 2026-11-15
branch: guard-limits
cost: { minutes: 224, tokens: 253525 }
id: l-073133bc
scope: repo
status: active
src: agent
touches:
  - core/engine/rust/src/budget.rs
  - core/engine/src/NoteRoute.h
  - core/engine/rust/src/notes.rs
  - core/engine/src/core_cpp.cpp
---

## Decided
- The per-block limit sent through oscairn.voice-budget is the admitted voices (the host's unstolen notes times voices per note) plus the room the soft limit leaves, never below the admitted voices unless the lane is measured over its limit, then at most a quarter fewer a block (VoiceBudget::voiceLimit, budget.rs voice_limit).
- Why: the old max(1, 0.75*voices, voices+room) added the room to the voices sounding before the block, while room already counts the admitted notes, so a chord from silence was capped at the room left after it: eight notes gave two voices (guard test, burst case).
- A shared instance's sounds get limits of their own: each context's admitted voices plus its share of the instance's room, so one sound's tails are not given up for another's notes while the instance keeps within its limit.
## Watch out
- Plugins without the extension are unaffected: admission counts each note at its learned cost and refuses a burst only when that estimate passes 85% of the lane's limit.
- The host reads active_voices instance-wide only; per-sound limits come from the host's own note ledger per context.
