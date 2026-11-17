---
title: worker-service counted services in a wall-clock window; now each tick is checked against the rule
author: Jacob
date: 2026-11-17
branch: ci-red
cost: { minutes: 29, tokens: 17734 }
id: l-8d228bdb
scope: repo
status: active
src: agent
touches:
  - core/engine/tests/worker_service.cpp
---

decided: the test judges each 5 ms tick by the clock it ran at: a tick that made a request (workerRequests moved) must end no sooner than due after the last service's tick start, one that made none must start before due after the last's end; due is workerServiceInterval, or 5x after 25 empty answers, reset by any other request (a parameter set). That the interval and the quiet interval were shown at all is required off CI, and relaxed only when the ticks came in time. ruled out: a lower count bound in a 400 ms window (macos-15 runner got 10 against 12: ticks oversleep). watch out: workerRequests is process-wide; a tick asserts it made at most one request, so another thread's request in a tick fails loudly rather than skewing; the model assumes the fixture answers services with nothing.
