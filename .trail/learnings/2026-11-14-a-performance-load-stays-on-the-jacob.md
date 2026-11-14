---
title: A Performance load stays on the control thread; safe requests are served during it through a gate
author: Jacob
date: 2026-11-14
branch: main
cost: { minutes: 190, tokens: 260651 }
id: l-11c0d8d0
scope: repo
status: active
src: agent
touches:
  - core/control/include/oscairn/control.h
  - core/control/include/oscairn/local_socket.h
  - core/control/src/local_socket.cpp
  - core/control/src/performance.cpp
  - core/control/src/session.cpp
  - hosts/standalone/tests/desktop_apps.cpp
  - core/control/CMakeLists.txt
  - core/control/tests/local_socket.cpp
  - docs/control-protocol.md
  - docs/plugin-porting.md
---

## Decided
- A load's plugin work is not moved to another thread. In-process CLAP instances bind to the first thread that controls them and refuse main-thread calls from any other, so an instance created or restored on a load thread could never be controlled by the control thread afterward.
- Instead the socket's helper thread serves an allow-list of safe requests (session.status, parts.meters, wires.meters, notes.send, notes.panic, subscribe, unsubscribe) while the load is inside a window that touches only objects the session does not know yet. One gate mutex means only one thread is ever inside the session.
- controls.set is NOT on the allow-list: a parameter change to a plugin in a worker waits on that worker's lock, which a load into the same shared plugin host holds for as long as its state loads; the helper would stall while holding the transport lock and no client would get even a busy answer.

## Tried and ruled out
- Moving the whole load to its own thread: ruled out by the CLAP thread binding above.
- Windowing a load that takes over an instance that is already playing: its host is shared with the playing graph, so it cannot be opened up this way. Measured with a 6 s slow load, one request every 20 ms: fresh instances 1032 of 1032 answered, take-over 1 of 348.

## Watch out
- A Setlist switch between Performances sharing an instrument is the take-over case, so clients still mostly see busy (-32009) there.
- Bringing controls.set back needs a worker setParameter that does not wait on the worker's lock.
- parts.meters answers during a load but holds its values from before it (the service tick does not run).

In the code: core/control/src/local_socket.cpp (gate, allow-list, helper), LoadWindow in the control library, windows in core/control/src/performance.cpp read(); core/clap-host/src/Host.cpp main() is the thread binding; tests control-not-blocked, -taken, -failing; docs/control-protocol.md "Long requests".
