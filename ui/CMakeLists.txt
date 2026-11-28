# The Rust UI (docs/rust-ui.md): built with cargo at the release rust-toolchain.toml pins, as
# samplepack is, into the build folder; installed as bin/oscairn-ui. A device's build
# (OSCAIRN_ROLAND_FANTOM0) or any other cross build (an OS image) draws straight to the display (the
# cargo feature kms); a build for the computer it runs on, a window (desktop). Cargo downloads its crates on the first build, as the JUCE options download JUCE,
# unless OSCAIRN_UI_VENDOR names a folder holding them (the device bundle's oscairn-ui-crates).
set(OSCAIRN_UI_VENDOR "" CACHE PATH "A cargo directory source with every crate ui/Cargo.lock names; cargo then runs offline")
set(ui_dir "${CMAKE_BINARY_DIR}/ui-cargo")
set(ui_features --no-default-features --features $<IF:$<OR:$<BOOL:${OSCAIRN_ROLAND_FANTOM0}>,$<BOOL:${CMAKE_CROSSCOMPILING}>>,kms,desktop>)
set(ui_options "")
# The release and the commit the JUCE apps are built with, which the Settings app's About page says (src/settings_keys.rs).
set(ui_env RUSTUP_AUTO_INSTALL=0 "OSCAIRN_RELEASE=${OSCAIRN_RELEASE}" "OSCAIRN_SOURCE_COMMIT=${OSCAIRN_SOURCE_COMMIT}")
if(OSCAIRN_UI_VENDOR)
    list(APPEND ui_options --offline --config "source.crates-io.replace-with='oscairn-vendor'" --config "source.oscairn-vendor.directory='${OSCAIRN_UI_VENDOR}'")
endif()
set(ui_binary "${ui_dir}/release/oscairn-ui")
if(CMAKE_CROSSCOMPILING)
    # For the target, linked by this build's C compiler (the toolchain's sysroot with it), its
    # system libraries found by this build's pkg-config (Buildroot's, at the target's staging).
    # The linker as a rustflag, not target.<triple>.linker: with --target, rustflags reach only
    # the target's code, so build scripts and macros still link for the build machine, whose
    # triple may be the same (an arm64 container).
    set(ui_triple "${CMAKE_SYSTEM_PROCESSOR}-unknown-linux-gnu")
    list(APPEND ui_options --target ${ui_triple})
    list(APPEND ui_env "CARGO_ENCODED_RUSTFLAGS=-Clinker=${CMAKE_C_COMPILER}" PKG_CONFIG_ALLOW_CROSS=1)
    set(ui_binary "${ui_dir}/${ui_triple}/release/oscairn-ui")
endif()
# Cargo decides what is stale; it leaves the binary alone when nothing changed.
add_custom_target(oscairn_ui ALL
    COMMAND ${CMAKE_COMMAND} -E env ${ui_env} ${SAMPLEPACK_CARGO} build --release --locked ${ui_options} ${ui_features} --target-dir "${ui_dir}"
    BYPRODUCTS "${ui_binary}"
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
    COMMENT "cargo build oscairn-ui"
    VERBATIM COMMAND_EXPAND_LISTS)
install(PROGRAMS "${ui_binary}" DESTINATION bin)
# Its tests: headless (the software renderer into a buffer, no window), a fake engine on a
# socket, and the real engine with no audio device when this build has it, and the JUCE app
# beside it to compare the session's setup with (session_setup_matches_the_juce_app).
set(ui_test_env "")
if(TARGET oscairn_engine_host)
    list(APPEND ui_test_env "OSCAIRN_ENGINE=$<TARGET_FILE:oscairn_engine_host>")
endif()
if(TARGET oscairn_standalone)
    list(APPEND ui_test_env "OSCAIRN_APP=$<TARGET_FILE:oscairn_standalone>")
endif()
add_test(NAME ui-rust COMMAND ${CMAKE_COMMAND} -E env ${ui_env} ${ui_test_env}
    ${SAMPLEPACK_CARGO} test --release --locked ${ui_options} ${ui_features} --target-dir "${ui_dir}"
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}")
set_tests_properties(ui-rust PROPERTIES TIMEOUT 900)
# The device bundle's list of the crates (os/buildroot/package/oscairn-ui-crates) matches Cargo.lock.
find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    add_test(NAME ui-crates COMMAND "${Python3_EXECUTABLE}" "${CMAKE_SOURCE_DIR}/os/scripts/ui-crates.py" --check)
endif()
