---
title: "A Performance not open counted every Sampler part at one stacked sound's busiest figure"
author: Jacob
date: 2026-11-17
branch: sharing-default
cost: { minutes: 1339, tokens: 2477857 }
id: l-951bcde5
scope: repo
status: active
src: agent
touches:
  - "core/control/src/budget.cpp#Session::measurementFor"
  - "core/control/src/budget.cpp#Session::costOf"
  - "core/control/src/budget.cpp#Session::estimate"
---

decided: measurements are keyed per declaration (id@version digest#declared bytes), and for the Sampler the declaration follows the loaded sound, so a key is effectively a sound. A Performance not open has no declarations, so measurementFor merged every key of the build with max(): two sounds measured stacked (cpu 0.51 at 24 voices) priced every Sampler part everywhere at 51 percent. Now the estimate ties a node's saved state to its key: read() digests each node's state folder (Session::stateDigest, FNV-1a over file names and bytes, empty past 16 MiB) into Prepared::states, open() moves it to stateOf_, and measure() (every 200 ticks, with the memory read) records states_[digest] = the key it plays under, saved as states in measurements.json while that key's figures exist. estimate(dir) uses that exact key for the part when the digest is known, else the merged record whose CPU fields are medians (lower middle) of the sounds that have CPU figures; memory stays the max (memory must err high). A busiest figure carries day (days since 1970, set when sampled); costOf ages it by half a week past the first week unused, never below rest + voice * voices; it is aged when read, never rewritten, so loads do not compound it. ruled out: identifying the sound from the document's assets (a library's sounds may share one pack file) or from a new field in performance.json (strict field lists; a format change for an estimate). watch out: a state edited and saved since it last opened has a new digest and counts at the median until it opens again; a Performance built with graph.add and never reopened likewise. control-parts outlierSound fails before the change (0.51 for ordinary parts) and passes after (0.025).
