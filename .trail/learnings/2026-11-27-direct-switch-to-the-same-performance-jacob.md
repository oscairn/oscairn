---
title: "Direct switch to the same Performance: free instances claimed beside instead of kept"
author: Jacob
date: 2026-11-27
branch: direct-switch
cost: { minutes: 13, tokens: 118198 }
id: l-0447c222
scope: repo
status: active
src: agent
touches:
  - "core/control/src/performance.cpp#Session::read"
  - core/control/tests/performance_switch.cpp
  - docs/control-protocol.md
---

decided: issue 9's slow direct loads (FANTOM: TSO 0.9-1.05 s direct, 0.35 s with an Empty between, the extra inside read, outside load and restore, 14-31 MB read() in the old Sampler workers, kept 0) were Session::read's claim (RFC 0021 sounds beside) running before take: a playing instance of the same plugin got the node's snapshot prepared beside its own sounds even when it was free (asleep or 3 s silent) and held that very node's state, so each Sampler parsed its sound again, re-resolved every sample over the worker's ResolveSample IPC and re-read every record's preload (checkRecord), buffers cached or not; and a claimed instance's origin had an empty source, so no later load could keep it, which is why only the second load after an Empty was fast (the first came by claim from ST Final). Mac, real Sampler 0.9.12 in chain workers, 5 parts x 400 store samples (TSO has about 2100): direct A->A 78-80 ms (beside ~50) before, 31-34 ms after (kept 5); A->B->A unchanged (beside 100-140 ms). Fix: claim skips a free instance whose origin source is one of this document's node sources (left to take, which compares with saveSnapshot+sameTree and resets); a claim now records source(key) so a later take compares (after a claim the Sampler's slots differ from the file, so that one take loads the state again, ~10% over a claim, then keeps). engine.timing open gained beside (prepareSnapshot beside a playing instance: claim, claimEngine, joining a playing engine). ruled out: a byte-equal post-claim snapshot (Sampler and Basic Synth put the beside sound in another slot, Default kept); caching ResolveSample answers in the worker (stale after a pack is rewritten). watch out: the fast path needs the instance free: a part fed by an arpeggiator is free only if the arp sleeps; the in-process Basic Synth's reset() returns false, so in-process it is given its state (still not beside); held or pedaled notes still go beside and ring out. Left for the Sampler (its repo): loadSamples' checkRecord reads the whole preload (header, table, window) of every record on every prepare even with cached buffers, and each sample is one IPC resolve; caching checked records per pack and offset and reading head_len only would cut the A->B cost.
