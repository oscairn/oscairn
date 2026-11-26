---
title: "Performance switch: worker processes started together ahead of the loads"
author: Jacob
date: 2026-11-26
branch: switch-time
cost: { minutes: 3892, tokens: 19602160 }
id: l-35e963cc
scope: repo
status: active
src: agent
touches:
  - core/plugin-host/src/WorkerHost.cpp
  - core/control/src/performance.cpp
  - core/plugin-host/include/oscairn/plugin_host.h
---

decided: issue 9's remaining switch time, measured on the Mac (5 Sampler parts + 5 Delays, workers, fresh Session each load, temporary timers since removed): of ~72 ms (saved separate) / ~90 ms (saved shared, split as it opens), worker process start was ~37-42 ms (10 starts at 3.5-4 ms each, one after another), plugin load ~9, state load ~8 (13 split), WorkerHost::record's recovery snapshot after each loadSnapshot ~9, asset resolve 2-7, estimate 2-3, publish 0.3. Session::read now counts the worker processes its loads may start (CLAP nodes placed in workers; with chain, one per placement group; less the free instances it may take over) and prestartWorkers spawns them all at once (Process::start split into spawn and started); loadPluginInWorker's own-process path and Pool::chain take a spare of the right kind (chain spares carry a slot as chain's do) and only await its Init reply; the read's end drops the rest (failed first, so killed at once, not asked to shut down with a 1 s wait). Mac: separate 72 -> 36 ms, shared 90 -> 57 ms. The CLAP main-thread binding is not touched: plugin loads stay on the control thread in turn; only the empty processes start together. ruled out: a resident spare kept between switches (memory per spare, and one spare does not cover the eight starts of a stage Performance). watch out: unused chain spares use up slot numbers, which shifts which audio core later chain processes land on (slots already grow across opens); an over-count spawns processes that are killed after the read; plugin hosts (OSCAIRN_WORKERS=pool / hosts) get no spares. Left: the recovery record save per state load (~10%), and the split's root loading every sound of a saved engine before the siblings (control-shared-parts' path).
