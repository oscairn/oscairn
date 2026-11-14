# The Rust UI (docs/rust-ui.md): built with cargo at the release rust-toolchain.toml pins, as
# samplepack is, into the build folder; installed as bin/oscairn-ui. A device's build
# (OSCAIRN_ROLAND_FANTOM0) draws straight to the display (the cargo feature kms); any other, a
# window (desktop). Cargo downloads its crates on the first build, as the JUCE options download JUCE.
set(ui_dir "${CMAKE_BINARY_DIR}/ui-cargo")
set(ui_features --no-default-features --features $<IF:$<BOOL:${OSCAIRN_ROLAND_FANTOM0}>,kms,desktop>)
set(ui_binary "${ui_dir}/release/oscairn-ui")
# Cargo decides what is stale; it leaves the binary alone when nothing changed.
add_custom_target(oscairn_ui ALL
    COMMAND ${CMAKE_COMMAND} -E env RUSTUP_AUTO_INSTALL=0 ${SAMPLEPACK_CARGO} build --release --locked ${ui_features} --target-dir "${ui_dir}"
    BYPRODUCTS "${ui_binary}"
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
    COMMENT "cargo build oscairn-ui"
    VERBATIM COMMAND_EXPAND_LISTS)
install(PROGRAMS "${ui_binary}" DESTINATION bin)
# Its tests: headless (the software renderer into a buffer, no window), a fake engine on a
# socket, and the real engine with no audio device when this build has it.
set(ui_engine "")
if(TARGET oscairn_engine_host)
    set(ui_engine "OSCAIRN_ENGINE=$<TARGET_FILE:oscairn_engine_host>")
endif()
add_test(NAME ui-rust COMMAND ${CMAKE_COMMAND} -E env RUSTUP_AUTO_INSTALL=0 ${ui_engine}
    ${SAMPLEPACK_CARGO} test --release --locked ${ui_features} --target-dir "${ui_dir}"
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}")
set_tests_properties(ui-rust PROPERTIES TIMEOUT 900)
