---
title: "Unused plugin versions: an unreadable source keeps every version; removal reads keptBy again before each version"
author: Jacob
date: 2026-11-28
branch: moves-185
cost: { minutes: 5153, tokens: 25273898 }
id: l-f81fe88a
scope: repo
status: active
replaces: l-6e06d640
src: agent
touches:
  - "core/control/src/performance.cpp#Session::installedPlugins"
  - "core/control/src/performance.cpp#Session::removeUnusedPlugins"
---

Decided: plugins.list's keptBy is computed in Session::installedPlugins from library documents, the open graph, Moves/*/move.json, plugin.json's kept mark and assets_.roots() folders, because each lives on disk and a cached copy would go stale between the preview and the removal. A source that cannot be read (a Performance or Sound document unreadable or over 4 MB, a saved Performance's folder whose drive is away, a kept move's unreadable journal) keeps every version as keptBy unreadable, because nothing says which version it needs and the owner's rule is that nothing a user made may become unreachable; a saved folder deleted from a parent that is still there counts as gone. removeUnusedPlugins calls installedPlugins again before each removal and is refused while a version job runs, because a move reads the old build. Watch out: an unreadable item blocks every removal until it is fixed or deleted; that is the intended trade.
