---
title: "The tests' sampler is a fixture in the plugins folder with the real Sampler's id"
author: Jacob
date: 2026-11-27
branch: test-sampler-fixture
cost: { minutes: 3910, tokens: 19964682 }
id: l-e4560b8c
scope: repo
status: active
src: agent
touches:
  - plugins/CMakeLists.txt
  - hosts/standalone/SoundPickerModel.cpp
---

Decided (#320, owner: just use a basic in-repo test plugin): core/engine/tests/test_sampler.cpp replaces the archived Legacy Sampler. It is built from plugins/CMakeLists.txt, not core, so it lands beside the other plugins: desktop_shell.cpp picks plugins by display name from the scanned plugin folder (plugin("Test Sampler")) and would miss it elsewhere. Its id is org.oscairn.sampler because SoundPickerModel decides importsSamples from that id and sound_picker.cpp's sample-import cases need it; the id is only in the build tree, never installed. It must list its files through oscairn.asset-dependencies (with the sha the store returns from resolve) or control-archives cannot see which library a Performance uses, and it must refuse unknown SFZ opcodes with 'line N: unsupported opcode X' because the picker test checks that message. Ruled out: extending pack-holder/Basic Synth (its patch type cannot carry a file name through the saved state) and building the new Sampler (Rust submodule per test build). Watch out: removing BUILD_TESTING (our own option) means -DBUILD_TESTING=OFF does nothing now. No test figure needed re-deriving: the kept-warm and memory-pressure assertions are counts and fractions of measured memory and passed unchanged.
