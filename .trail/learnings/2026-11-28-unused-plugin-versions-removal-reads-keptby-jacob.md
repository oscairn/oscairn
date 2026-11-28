---
title: "Unused plugin versions: removal reads keptBy again; an outside Performance keeps a version only while its folder is readable"
author: Jacob
date: 2026-11-28
branch: moves-185
cost: { minutes: 4969, tokens: 24981784 }
id: l-6e06d640
scope: repo
status: active
src: agent
touches:
  - "core/control/src/performance.cpp#Session::installedPlugins"
  - "core/control/src/performance.cpp#Session::removeUnusedPlugins"
---

Decided: plugins.list's keptBy is computed in Session::installedPlugins, not kept in a file, because every source it reads (library documents, the open graph, Moves/*/move.json, plugin.json's kept mark, assets_.roots() folders) already lives on disk and a cached copy would go stale between the preview and the removal. A kept move keeps the version it moved from, because versions.undo puts back items that need it. removeUnusedPlugins reads installedPlugins again before each removal, because an item saved between the preview and the removal must keep its version, and it removes through removePlugin(..., evenIfUsed=false) so package dependents still refuse. Watch out: performance.save records asset references only (assets_.addRoot), so a Performance saved outside the library whose drive is away keeps no plugin version; making it keep them needs the plugins recorded at save time in Session::savePerformance.
