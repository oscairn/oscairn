---
title: "The publisher's claim (RFC 0043 step 1): refused when malformed, kept only from a whole-archive signature by a build signer"
author: Jacob
date: 2026-11-28
branch: moves-177
cost: { minutes: 4926, tokens: 24951524 }
id: l-e3b5e7bb
scope: repo
status: active
src: agent
touches:
  - "core/control/src/package.cpp#readPackage"
  - "core/control/src/performance.cpp#Session::installUnpacked"
  - tools/oscairn-sign/oscairn_sign.cpp
  - tools/package-repository/template/scripts/build-index.py
---

Decided: a malformed claim (opensStateFrom not text, versions not dotted numbers of 1-4 parts, opensStateFrom newer than the version, soundChanges empty, over 1,000 bytes, or without opensStateFrom) makes readPackage refuse the whole archive, as every other manifest error does; a well-formed claim the device cannot trust (archive unsigned, or library.sig's key signed none of that plugin's builds in the archive) is installed but not kept in plugin.json. Why: the RFC's gate says refused for the first group and not kept for the second; refusing an untrusted claim would make an unsigned plugin package with a claim uninstallable for no gain. storeBuild now carries the recorded claim and license through every rewrite of plugin.json, so plugins.addBuild or a later archive without a claim never drops it; a later install with a kept claim replaces it. The index builder gained each version's plugins list (format, id, version, platforms, opensStateFrom) because RFC 0031's per-version plugins did not exist yet and the claim had nowhere to go; the device does not read it yet and the builder's one-publisher-per-plugin checks are not built. Watch out: a claim kept with a version says nothing on its own: the move (step 2) must also check that claimKey signed both the new build here and the old build, since an install keeps a claim even when its builds were already in the store from another signer.
