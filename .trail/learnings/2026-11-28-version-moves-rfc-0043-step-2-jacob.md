---
title: "Version moves (RFC 0043 step 2): state rewritten in a copy by an offline job, the whole item kept under Moves/, swapped on the control thread"
author: Jacob
date: 2026-11-28
branch: moves-179
cost: { minutes: 4720, tokens: 24540143 }
id: l-71e1bbf4
scope: repo
status: active
src: agent
touches:
  - core/control/src/versions.cpp
  - "core/engine/src/PreparedInstrument.cpp#preparedInstrument"
  - core/clap-host/tests/Fixture.cpp
---

Decided: the move saves each node's state again with the new version (owner 2026-11-28 on #259: the kept-warm take matches a state against its file), so RFC 0043's stateSavedBy is not written and undo cannot be 'restore one JSON file': the job (versions.cpp, its own thread) copies the item into a .move-<pid>-... staging folder in the data folder, rewrites the copy (node snapshots via the new build's saveSnapshot, plugin version and pathHint, lock, revision and content hash through writeInfo/contentHash), and the control thread, when versions.job first sees the job finished, renames the live folder to Moves/<move>/<id> and the copy into its place, only if the library still has the revision the job read (else savedSince). Hosts come from loadPlugin(..., ProcessingMode::Offline): with a worker program that is always a worker of its own (never a pool or chain host shared with the playing graph), with the policy's memory limit and reply timeout; in-process they are created, used and destroyed on the job thread only, which the CLAP thread binding allows. Watch out: an instrument's node snapshot is PreparedInstrument's 'pool 1' format with each instance in a numbered subfolder, so the job must wrap the loaded host with preparedInstrument() exactly as the graph does, else loadSnapshot fails with 'Unsupported parameter-only snapshot'. The test fixture gained kick-clap-fixture-v2, -v2_changed (reads its gain at half) and -v2_ports (stereo out) as version 2 builds; KICK_FIXTURE_REFUSE_STATE with a count of 1 refuses just the first state load, which is how one item of a move is refused while the rest moves. Not built: shared-engine nodes are moved by their engine's first node only and their ports are not compared; the claim's repository-index route for an old version not installed.
