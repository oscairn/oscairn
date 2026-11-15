---
title: "What a restarted worker forgot: values, modulation and controllers sent per channel"
author: Jacob
date: 2026-11-15
branch: shared-restart-restore
cost: { minutes: 56, tokens: 52913 }
id: l-a1e656ed
scope: repo
status: active
src: agent
touches:
  - "core/plugin-host/src/WorkerHost.cpp#replay"
  - core/control/tests/shared_parts.cpp
---

After a worker restart a shared instance's sounds came back with other values (the joiners at the lead's): restore replayed the instance's last published parameter values as edits to every channel, and nothing replayed what had been sent per channel (each sound's bus event, edits, control values, modulation) or the controller state a plugin keeps only in memory (mod wheel, pedals, bend, pressure). Decided: the WorkerHost, where every event crosses to the worker, tracks on the audio thread each channel's last controllers, bend, pressure and channel-wide expressions and each parameter's last value and modulation per channel (slot 0 for all), forgets a channel's when it selects another sound (selectSound bumps a counter the audio thread reads) and everything at a state load, and replays them at the start of the first block after the restart, behind the restored selections; it tracks while the worker is down too. The global values replay is skipped when the channels select different sounds. Ruled out: replaying each SoundContext's values_ from the session, because values from controls and lanes and all modulation reach the plugin as graph events that never pass values_, and a lead made from a node playing alone holds the Default's values while its queued selection is pending (soundValues gives none for a selected sound). Notes are not replayed: a note restarted from its attack sounds wrong, the controllers keep the next notes right. Limit: edits through setParameter to all channels before an instance came to be shared are lost on restart.
