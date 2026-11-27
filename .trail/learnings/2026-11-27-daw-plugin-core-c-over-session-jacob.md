---
title: "DAW plugin core: C++ over Session, its own control thread, state as the saved Performance folder"
author: Jacob
date: 2026-11-27
branch: daw-core
cost: { minutes: 29, tokens: 29671 }
id: l-d4ff975f
scope: repo
status: active
src: agent
touches:
  - hosts/plugin-core/plugin_core.cpp
  - hosts/plugin-core/include/oscairn/plugin_core.h
  - hosts/plugin-core/tests/plugin_core.cpp
---

## Decided
- The DAW plugin's core (RFC 0047 step 1, hosts/plugin-core) is C++, not Rust: everything it calls is the C++ Session (libraryQuery, loadPerformanceFromLibrary, loadPerformance, savePerformance, process, service, setSampleRate). A Rust core would be an FFI layer over that whole API, more code than the core itself; the Rust engine core and CLAP host already sit beneath Session. Format shells (CLAP C, VST3 C++, AU ObjC++) can all call it directly.
- Each Instance owns one Session on a control thread of its own (tasks in order, service() every 5 ms). In-process CLAP instances bind to the first thread that controls them (learning l-11c0d8d0), so every Session call goes through that one thread; the DAW's main thread posts picks and restores (async, status() polled) and waits for library() and state(). process() is the only call on the audio thread: no locks, events copied into a fixed array.
- State: "OSPS", version 1, the identity (library ID, name), then the files of the folder Session::savePerformance writes, by relative path. Packages and samples stay by reference because that is what the document holds. Restored folders are kept for the instance's life: a stand-in for a plugin not installed re-reads its saved state from there at the next save, so a project from another machine keeps the missing plugin's place and state.
- Missing report = the session's failures right after the load (stand-ins: "<id> <version> is not here (not installed)...") plus notices added by it (optional samples). A required sample missing refuses the load (Performance: missing sound assets); the state that did not open is kept and handed back byte for byte by state(), so saving the project loses nothing.
- Workers: Config::worker sets the process-wide placement policy if none is set. Placement is global, so the first instance wins; #303 must own this properly.
## Tried and ruled out
- Saving into folders inside the instance's work folder: Session::savePerformance adds the folder as an asset-store root (assets.cpp addRoot), and roots whose parent is gone are kept as "away", so the roots file would grow by one per DAW session. Saves go in sibling folders under one shared temp/oscairn-plugin folder that stays; a deleted one is pruned at the next save.
## Watch out
- state() during playback does a whole-state save of each plugin; in workers that can cost a control silence and reset (learning on the switch cut to digital zero). DAW autosaves will hit this; #300 should look at it.
- Several instances in one DAW process share the Library, the asset store and the placement policy; only sequential instances are tested.
- Plugins with their own windows will need the real main thread on macOS (#304); the core's control thread is not it.
