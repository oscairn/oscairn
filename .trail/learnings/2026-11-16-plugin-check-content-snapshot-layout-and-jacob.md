---
title: "plugin-check content: snapshot layout and sound-open rules"
author: Jacob
date: 2026-11-16
branch: check-content
cost: { minutes: 29, tokens: 71974 }
id: l-187a7173
scope: repo
status: active
src: agent
touches:
  - ui/src/tests.rs
  - ui/src/tests/routing.rs
  - docs/rust-ui.md
  - ui/PROGRESS.md
  - ui/slint/loading.slint
  - ui/slint/part.slint
  - ui/slint/shell.slint
  - ui/slint/theme.slint
  - ui/slint/topbar.slint
  - ui/src/client.rs
---

Found while adding --state/--asset to oscairn-plugin-check. A saved Performance keeps a node's state at nodes/<key>/<stamp>/state/state.clap (a Sound's at nodes/<key>/state/state.clap), so replace every state.clap file under nodes/, not a fixed path. A plugin with sound contexts (the Sampler) saves a native-sound snapshot (header oscairn-sampler-contexts/3), not its plain oscairn-sampler-state text; a hand-made plain state was rejected by 0.9.4 ('Plugin rejected state'). The export carries samples only if the plugin lists them in oscairn.asset-dependencies at save time, so the state must be loaded before the save the export uses. A library Sound opens only through resolvePlugin (performance.cpp): pathHint, then the plugin store at the saved version, then bundled; the version must equal the node's saved version exactly, and the node's saved audio ports must match the plugin's (audio port layout changed). A plugin package holds plugins and no items (package.cpp:533).
