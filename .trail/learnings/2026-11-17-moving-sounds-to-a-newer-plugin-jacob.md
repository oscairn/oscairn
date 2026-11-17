---
title: Moving sounds to a newer plugin version opens them through openNodes on the control thread
author: Jacob
date: 2026-11-17
branch: version-moves
cost: { minutes: 1350, tokens: 2196592 }
id: l-b77d53c4
scope: repo
status: active
src: agent
touches:
  - core/control/src/versions.cpp
  - "core/control/src/performance.cpp#Session::openNodes"
  - "core/control/src/package.cpp#claimKey"
---

decided: RFC 0043's plan and move check (each item's nodes opened by the new version) is Session::openNodes: the document's nodes of the plugin loaded into graph_.prepareReplacement(), a graph never published, restored as opening does (savedSound for shared instances, loadSnapshot otherwise), ports compared. It runs on the control thread, one item per service() (advanceVersions), not on a job thread as the RFC drew it: in-process CLAP instances bind to the thread that made them (the Nov 14 learning on Performance loads). parts.useVersion reuses openNodes with keep: the opened Graph::Node goes in with Graph::restore under the old NodeId, the path undo already uses, so edit.undo puts the old host back with no new code. A shared instance moves whole: naming one node moves every node on its engine onto one new instance. ruled out: comparing settings against the document for a plugin with state: performance.json holds no parameter values then, so changes are computed only when the old version is installed too (opened the same way); a claim for a saved version that is not installed cannot be checked against its publisher until the device reads the index's per-version plugins (RFC 0031 step 9), so such items are noClaim and move alone with withoutClaim. watch out: a parameter-only snapshot (no clap.state) is refused by a version that added a parameter (PluginHost::loadSnapshot wants every writable parameter), so such a plugin cannot keep the opensStateFrom promise across a new parameter; Performance 14 and Sound 9 took the versions RFC 0031 had planned for pluginRepositories.
