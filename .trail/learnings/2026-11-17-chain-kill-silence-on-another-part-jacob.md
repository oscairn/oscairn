---
title: Chain-kill silence on another part under TSAN was its own missed deadlines
author: Jacob
date: 2026-11-17
branch: ci-red
cost: { minutes: 281, tokens: 604088 }
id: l-ed4821c5
scope: repo
status: active
src: agent
touches:
  - "core/engine/tests/plugin_groups.cpp#killOneChain"
  - core/test-support/relaxed.h
  - core/control/tests/multi_sampler_load.cpp
---

decided: under relaxed(), another part's silent held block counts against the kill only when no plugin on its path (fm0 the shared instance, its sound context, its four inserts) counted a deadline miss from the block before to the one after; part 3's loudness compared only when its path missed none around the blocks compared. Off CI unchanged. Evidence: under TSAN plus load every chain's statistics showed silent blocks before the kill too (master 52, p4eq2 21 by block 363). ruled out: a window of this block and the one before only, and the part's own sound context without fm0: a run still had an uncounted silence (port 5, block 419); with fm0 and the block after, none in 5 runs. Also: sanitized() in relaxed.h (sanitizer build only) gates control-multi-sampler-load's 1 MiB-declaring sampler cases, whose worker limit (declaration x1.5 + 64 MiB = 65 MB) is under a sanitized worker's own memory. watch out: control-performance-switch line 394 still uses relaxed() for a sanitizer-only reason (the floor case); sanitized() fits it.
