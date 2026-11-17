---
title: A build added to a plugin version already in the store must share a signer with what is installed
author: Jacob
date: 2026-11-17
branch: unit-findings-4
cost: { minutes: 1566, tokens: 2630733 }
id: l-53b1db49
scope: repo
status: active
src: agent
touches:
  - core/control/src/performance.cpp
  - core/control/src/packages.cpp
  - core/control/tests/archives.cpp
  - docs/library-archive.md
  - docs/control-protocol.md
---

## Decided
- How the unit's foreign build got in: Session::installUnpacked judged each build only against the trust store. The install passed acceptUnsignedCode (library.contents listed that one build as unknown code, the testing key being in no trust store), which let the testing-key macOS build of org.oscairn.sampler 0.9.5 through. storeBuild then added it, because the store held only the linux-arm64 build and builds are matched by platform and digest, never by signer.
- Now: the signatures beside a plugin version's installed builds (<bundle>.sig) pin it. A build for a platform not yet there must carry one of those keys, whatever acceptUnsignedCode says. Otherwise it is noted ("<name> <version> (<platform>) is signed by <key>, not by <key> (<name>), who signed the ... installed here; it was not installed"), left out of `code` and of the unknown-code question, and never stored. The rest of the archive installs. A version installed unsigned pins no one.
- `replace` (a repository build over this platform's build, the user's explicit choice) is not held to the pin. control-repository relies on it: the user's own build, signed by another key, is replaced by the publisher's.
## Tried and ruled out
- The other route in the brief, a key trusted for that publisher ID: the trust store records scopes, not publisher IDs, so there is nothing to look up. Key replacement (RFC 0006) is not built either.
## Watch out
- archiveContents' `code` still lists such a build as unknown code. Only the install refuses it.
