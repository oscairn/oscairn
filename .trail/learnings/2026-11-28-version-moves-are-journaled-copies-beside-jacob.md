---
title: "Version moves are journaled: copies beside move.json first, every rename settled from the disk at start"
author: Jacob
date: 2026-11-28
branch: moves-179
cost: { minutes: 58, tokens: 76218 }
id: l-eb39cfee
scope: repo
status: active
src: agent
touches:
  - "core/control/src/versions.cpp#Session::versionJob"
  - "core/control/src/versions.cpp#Session::recoverVersionMoves"
---

Decided: a move first renames each staged copy into Moves/<move>/<id>.new, then writes move.json durably (state swapping; file fsync, rename, folder fsync), then per item renames the live folder to Moves/<move>/<id> and <id>.new into its place, then writes state done. settleSwap reads only the disk, so it is safe to repeat: live folder and kept folder both there means swapped, only the live folder means not started, only <id>.new means finish forward, only the kept folder means roll back. Undo records undoing:[ids] the same way. Session's constructor settles any move left swapping or undoing, because a crash between two renames otherwise left the item out of the library and its bytes under a folder nothing lists. Journal paths are relative to the data folder because the reviewer's crash test copies data folders, and an absolute path then pointed a settle at another folder. Watch out: fs::path(x).begin() and fs::path(x).end() on two temporaries loop forever (that hung every test once); the crash test re-runs the test binary with OSCAIRN_MOVE_CRASH_AT=N, which _Exit(86)s at the Nth journal step.
