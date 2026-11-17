---
title: "The engine core's C++ twin deleted; its outputs replayed from recorded vectors"
author: Jacob
date: 2026-11-17
branch: core-twin-removal
cost: { minutes: 1401, tokens: 2316002 }
id: l-59242cb5
scope: repo
status: active
replaces: l-3fe41bda
src: agent
touches:
  - core/engine/tests/vectors.h
  - core/engine/tests/vectors_math.c
  - core/engine/tests/core_diff.cpp
  - core/engine/tests/audio_diff.cpp
  - core/engine/tests/run_diff.cpp
  - core/engine/src/engine_core.h
  - core/engine/src/graph.cpp
  - core/engine/src/NoteRoute.h
  - core/CMakeLists.txt
---

## Decided
- The engine core's C++ twin (core_cpp.cpp, audio_cpp.cpp, run_cpp.cpp, the cpp_core_ table, OSCAIRN_ENGINE_CORE, NoteRoute.h's VoiceBudget) is deleted under the owner's three rules: Rust proven on the keyboard, the C++ outputs frozen first, one clean bundle after the switch. engine-core-diff, engine-audio-diff and engine-run-diff now replay core/engine/tests/vectors/*.vec, recorded from the twin at bd57eed7 (commit 271f6e9a is the last with it; check it out to run the twin again).
- A vector file holds, per plan, a running 64-bit digest of every value the test compares: the top 16 bits at each block's end (locates the first differing block), the whole 64 bits every 64th block and at a plan's last (strength). 1.71 MB for 805,000 blocks.
- The test executables define exp, log1p and pow (core/engine/tests/vectors_math.c, IEEE arithmetic alone), which the Rust core's curve shapes and zone velocity curves call instead of the platform libm, and a test fails if the core never reached them. macOS's pow(x, 2) is not the correctly rounded x * x for 0.13 % of x in 0..1, so vectors recorded with a libm would replay only on that libm. The tests are built -ffp-contract=off so the generators' own arithmetic does not depend on compiler, target or level.
- A change to the core that changes what the tests see needs new vectors recorded from the Rust core (`record FILE COMMIT`) with a reviewed reason in the commit (docs/developer-guide.md, The engine core's vectors).
- The layout static_asserts that lived in core_cpp.cpp moved to graph.cpp; the 2^16 voice-limit clamp test moved from note_routing.cpp to budget.rs.

## Tried and ruled out
- Recording per platform: needs the twin on each platform, and Linux could not be run here (no Rosetta, no VM allowed), and the twin is gone now.
- Regenerating bindings.rs with the local bindgen (CommandLineTools libclang): it rewrites every layout test's formatting and the clap-host bindings too; only MADE_FROM's first hash was edited, as before.

## Watch out
- A generator draw inside a call's arguments or an operator's operands differs by compiler: GCC on x86_64 evaluates right to left, clang left to right, so plans (and replay) differed on x86_64 GCC until every draw got its own statement, in clang's order (the vectors stayed byte-identical). Braced initializers are sequenced. Keep new draws one per statement.
- The audio test feeds NaN, -inf and 2e6 on purpose; an unrendered block passes them unguarded into later mixes, where inf - inf or inf * 0 makes the CPU's default NaN (sign set on x86, clear on arm64). Samples and meters are digested with every NaN as 0x7FC00000 (vectors::canonical).
- Interposing exp/log1p/pow relies on the executable's definitions winning over libm's at link time: seen with ld64 and GNU ld; if a platform fails with the "never vectors_math.c's" message, that is why.
- core/plugin-host/src/Worker.cpp and worker_shared.h still name audio_cpp.cpp in comments (another session's files); worker_shared.h is hashed into the engine bindings' MADE_FROM.
- graph.cpp's Db curve is still exp(db / 20 * ln 10), not pow(10, y), as the Rust run's (clang turns pow(10, y) into exp10 from -O1); parts.cpp evaluates it on the control thread through curveOf(...).point.
