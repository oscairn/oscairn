---
title: "A worker's memory leaves out the file pages every worker shares"
author: Jacob
date: 2026-11-16
branch: rust-host-rss
cost: { minutes: 811, tokens: 1237064 }
id: l-df7cb934
scope: repo
status: active
src: agent
touches:
  - "core/plugin-host/src/plugin_host.cpp#workerOwnBytes"
  - core/plugin-host/src/WorkerHost.cpp
  - "core/control/src/budget.cpp#workerCode"
---

## Decided
The Rust CLAP host's TSO Carol Bells refusal (410 MiB needed, 404 allowed) was accounting, not use: a worker's memory was its whole RSS (/proc/PID/statm), and a locked worker holds its binary and libraries resident, pages every worker shares (one copy in RAM, Pss equal on both hosts). The Rust worker binary is 1 MB larger, so each worker was charged 1 MB more. Now, on Linux, a worker's memory is RSS less the RssFile it held right after Init (Process::start, before any plugin): workerOwnBytes. The binary's loadable segments (workerCodeBytes, read from its ELF headers) are counted once in reservedMemory (or taken off the flat budget); the libraries are the engine's too, in the system share. measurements.json went to version 6: a version 5 file keeps its CPU, never its memory. macOS is unchanged (phys_footprint already leaves clean code out).

## Tried and ruled out
Pss per worker: it depends on how many processes share a page when it is read, so a stored per-plugin figure would under-count a Performance opened with fewer workers. smaps on every 100 ms reading: too costly with 20 workers on an A53; RssFile is read once per worker start, statm stays the reading.

## Watch out
In Docker Desktop, a binary on the bind-mounted /src (virtiofs) loses resident pages even while mlocked (VmFlags lo, Locked 320 of 1500 kB): measure workers copied to the container's own /tmp. Over-count left: a plugin's own .so shared by several workers of that plugin is counted in each (Sick tines' 20 effects workers).
