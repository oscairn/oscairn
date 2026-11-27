---
title: The boot supervisor restarts a crashed engine; a same-bundle stop and start raced it
author: Jacob
date: 2026-11-27
branch: supervisor-restart
cost: { minutes: 3973, tokens: 20630609 }
id: l-38922e25
scope: repo
status: active
src: agent
touches:
  - devices/roland-fantom0/bundle/oscairn-fantom-boot
  - devices/roland-fantom0/bundle/oscairn-fantom-start
---

Cause of the 2026-11-27 incident (issue 327): oscairn-fantom-start removes var/oscairn/run/stopped early, before the app, engine or UI run, so a hand stop then start in the SAME bundle left the supervisor (oscairn-fantom-boot watch) seeing no process and no mark for seconds, and it took the unit to Roland's application. A swap to another bundle is not hit: the old bundle's mark stays. Decided (owner accepted): a crash is restarted (3 within 60 s, OSCAIRN_BOOT_RESTARTS and OSCAIRN_BOOT_RESTART_WINDOW), then the fallback; the supervisor also waits while the bundle's start.lock exists (given up after the start's limit, so a lock left by a killed start cannot keep the unit with nothing running), which is what makes oscairn-fantom-restart and a hand stop and start safe, so no restart flag file was needed. The start records run/cold so a restart repeats --cold when the power-on start was. Ruled out: a flag file with a timeout (the lock and mark already say it, and a flag would go stale); counting restarts made through oscairn-fantom-restart. Not tried on the unit.
