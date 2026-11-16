---
title: "switch-stress-hosts 'connection lost' was Session::measure asking a dead worker for its room"
author: Jacob
date: 2026-11-16
branch: memory-fixes
cost: { minutes: 506, tokens: 640868 }
id: l-fd60b039
scope: repo
status: active
src: agent
touches:
  - "core/control/src/budget.cpp#measure"
  - core/control/tests/measure_lost_worker.cpp
---

control-performance-switch-stress-hosts failing with 'FAIL: Plugin worker connection lost' (or '... closed') came from b7e8d68a: Session::measure() called SoundContext::room(), which asks the worker (WorkerHost::soundContexts, a request) every tick for each shared-instance node, outside any try. The stress test SIGKILLs workers at random; when one died between sample()'s failure() check and that request the exception escaped Session::service() through the test's serve() (outside its try), ending the run. Found with a backtrace printed at Worker.cpp's writeAll/readAll throws: Session::measure -> SoundContext::room -> WorkerHost::soundContexts. Not a memory-limit kill, not -dead_strip (no unexpected worker crash reports), not more workers: under the same load (ctest -j3 control- beside it) main, 124e9996 and 87a49934 failed 0 of 40 each, b7e8d68a 26 of 40 (alternating runs, load average about 21 on a 36-core Mac). Fix: catch around the room request as declaredBy() does; anything in measure() that asks a worker must not throw out of service(). control-measure-lost-worker reproduces it deterministically by SIGSTOPping the worker with OSCAIRN_WORKER_REPLY_SECONDS=1 and OSCAIRN_WORKER_SERVICE_MS=1000 (so the plugins' own service requests wait and measure asks first).
