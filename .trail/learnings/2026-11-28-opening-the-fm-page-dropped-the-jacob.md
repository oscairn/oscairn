---
title: "opening the FM page dropped the engine connection: params.list's answer sent a params.text per value of every stepped parameter, written from the UI thread wit…"
author: Jacob
date: 2026-11-28
branch: fm-page-connection
cost: { minutes: 4830, tokens: 24831848 }
id: l-141eb61b
scope: repo
status: active
src: agent
touches:
  - ui/src/client.rs
  - "ui/src/params.rs#ask_text"
---

Cause: params::Screen::listed -> ask_values asks the text of every value of every stepped parameter of up to 64 values (an FM plugin: hundreds) in one go; shell::send wrote each on the UI thread with a 200 ms write timeout, and a write that timed out part way shut the connection (client.rs write) because a cut frame cannot be mended. With the engine not reading (a fake engine in a walk test, a busy engine), the unix socket buffer (about 8 KB, some 70 requests) filled. Fix: the client queues requests to a writer thread of its own (blocking writes, no timeout, a full queue of 4096 drops the request, never the connection), and the page keeps at most 32 params.text out, the rest waiting for answers. Test: shellpages::the_fm_page_opened_from_the_shell_keeps_the_connection (it failed with 'the connection was lost' before).
