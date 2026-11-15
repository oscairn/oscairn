---
title: "The Rust UI's crates in the bundle build"
author: Jacob
date: 2026-11-14
branch: rust-ui-setup
cost: { minutes: 82, tokens: 132897 }
id: l-3e828aad
scope: repo
status: active
src: agent
touches:
  - ui/CMakeLists.txt
---

Decided: the bundle build gets oscairn-ui's crates from the oscairn-ui-crates package, every crate in ui/Cargo.lock as a Buildroot extra download from static.crates.io, checked against the lock's own checksum (the .crate file's SHA-256) in its .hash, unpacked as a cargo directory source with a .cargo-checksum.json of an empty file list; cargo runs --offline with source.crates-io replaced (OSCAIRN_UI_VENDOR). Ruled out: Buildroot's cargo-package vendoring, which runs cargo vendor on a downloaded source archive (the local oscairn tree has none), and a checked-in cargo vendor tree (700 MB unpacked). Cargo needs every lock entry present, Windows and macOS crates too, because resolution is not per target, so the set cannot be trimmed without stubs. After any Cargo.lock change run os/scripts/ui-crates.py; ctest's ui-crates fails until then. Watch out: on an arm64 build machine the host and target triples are the same, so target.<triple>.linker (or CC_<triple>) would link the build scripts against the target's glibc and they fail to run (GLIBC_2.43 not found); the linker goes in CARGO_ENCODED_RUSTFLAGS, which with --target reaches only the target's code.
