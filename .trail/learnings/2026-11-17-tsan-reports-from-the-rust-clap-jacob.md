---
title: "TSAN reports from the Rust CLAP host were Rust's uninstrumented atomics; tests failed from the reports' cost"
author: Jacob
date: 2026-11-17
branch: ci-red
cost: { minutes: 1447, tokens: 2365317 }
id: l-065c929d
scope: repo
status: active
src: agent
touches:
  - "core/clap-host/rust/src/host.rs#Gate"
  - core/clap-host/rust/build.rs
  - core/CMakeLists.txt
---

decided: the Rust host's gate (Gate::new/drop and process()'s swap/store) calls __tsan_acquire/__tsan_release under cfg(oscairn_tsan), set by build.rs when OSCAIRN_TSAN=1, which CMake adds to engine_core_env when CMAKE_CXX_FLAGS has -fsanitize=thread. Without it ThreadSanitizer saw the plugin's activate (main) and start_processing/process (audio) and the host's port buffers as races, because the handover is a Rust AtomicBool it cannot see. The twenty-odd test failures beside the reports (worker-recovery 0.75, control-source-workers silent, sampler-workers aborted) were the reports' cost: TSAN_OPTIONS=report_bugs=0 made them pass with no code change, and the annotation made them pass with no reports. ruled out: switching the TSAN job to the C++ host (loses the default host's coverage). watch out: other Rust-side handovers (SeqCst flags, the queues) are not annotated; if new reports name them, annotate the same way. Repro: CI's configure in an Ubuntu 24.04 aarch64 container (osc-ci-arm64, Rust copied from pcm-rust-img, RUSTUP_TOOLCHAIN pinned since the toolchain file asks for components offline), source copied into the container's own filesystem.
