---
title: "A shared instance restored from one joiner's snapshot after a worker restart"
author: Jacob
date: 2026-11-15
branch: shared-restart-restore
cost: { minutes: 30, tokens: 30829 }
id: l-5432aa06
scope: repo
status: active
src: agent
touches:
  - "core/plugin-host/src/WorkerHost.cpp#prepareSnapshot"
---

After a worker restart a shared instance opened from a saved Performance, or taken over by a Performance change, played one sound on every channel. Cause: WorkerHost::prepareSnapshot made a copy of the snapshot just prepared beside the playing sounds its recovery record, dropping the record of the instance and the sounds prepared before. That was right for an unshared Performance change (the incoming snapshot replaces the instance) but every joiner prepared from a snapshot (joinEngine, a claimed engine) goes through it, so the record became the last joiner's snapshot alone and the selections no longer matched any restored sound. Decided: after a prepare beside, the record is a fresh SaveSnapshot of the whole instance, the existing sounds and the incoming ones, with base_ read back from contexts(). Watch out: this saves a snapshot of a playing instance at each joiner during a load, which is extra file work on the device.
