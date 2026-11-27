cmake_minimum_required(VERSION 3.28) # FetchContent_Declare EXCLUDE_FROM_ALL (plugins/), set_property(TEST ... DIRECTORY) below
project(oscairn VERSION 0.1.0 LANGUAGES C CXX)
# A release is named by this version and this label while there is one (docs/releasing.md):
# Oscairn-0.1.0-alpha.1-macos-arm64.dmg, tag v0.1.0-alpha.1. Empty for a final release.
set(OSCAIRN_PRERELEASE "alpha.1")
# The release, with its label: what the app, its apps, the CLAP host and the engine say they are
# (OSCAIRN_RELEASE in each). Info.plist keeps the numeric version, as macOS wants.
set(OSCAIRN_RELEASE ${PROJECT_VERSION})
if(OSCAIRN_PRERELEASE)
    string(APPEND OSCAIRN_RELEASE "-${OSCAIRN_PRERELEASE}")
endif()
# The commit a release is built from (tools/release-macos.sh sets it), for About; empty: a development build.
set(OSCAIRN_SOURCE_COMMIT "" CACHE STRING "The commit this build's source is, for a release; empty for a development build")
if(APPLE)
    enable_language(OBJCXX) # the CLAP host's plugin windows (core/clap-host/src/Window.mm), and JUCE
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE ON) # the static core libraries link into Oscairn.clap, a shared module (thread_local needs it)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# The JUCE desktop app and audio/MIDI helper. Off by default: these download JUCE
# and need platform device libraries. The core never depends on JUCE.
option(OSCAIRN_STANDALONE "Build the desktop app (downloads JUCE)" OFF)
option(OSCAIRN_ENGINE "Build the separate audio/MIDI engine helper (downloads JUCE)" OFF)
option(OSCAIRN_MODULAR "Build the Modular instrument plugin (downloads JUCE)" OFF)
option(OSCAIRN_LV2 "Enable LV2 hosting (requires Lilv and LV2 development packages)" OFF)
# Optional hardware is compiled out of products without it: on for an image whose profile has a
# panel of protocol roland-i2c-slave (the FANTOM-0), which the engine then reads; the reader is
# built for its test either way, but linked into nothing else.
option(OSCAIRN_ROLAND_FANTOM0 "Read a Roland FANTOM-0's panel and keys in the engine: the FANTOM-06, -07 and -08 (Linux)" OFF)
# The audio path (core, hosts/audio, hosts/engine: the engine, the plugin worker and what they link) at -O3
# over the toolchain's -O2: the device's cores run the engine thread's code about 2.4 times faster. The app's
# own sources and the plugins keep the build's level; no floating-point option changes.
option(OSCAIRN_AUDIO_O3 "Compile the audio path at -O3" OFF)
set(OSCAIRN_LV2_EXAMPLE_DIR "" CACHE PATH "Optional upstream LV2 example bundles for integration tests")
set(OSCAIRN_LV2_SOURCE_DIR "" CACHE PATH "Optional LV2 source checkout for unmodified eg-sampler Worker tests")

enable_testing()

# JUCE, for the app, the engine and Modular, declared once here so every user gets the same
# patched copy (hosts/juce-patches); fetched only by a component that needs it.
include(FetchContent)
FetchContent_Declare(JUCE
    GIT_REPOSITORY https://github.com/juce-framework/JUCE.git
    GIT_TAG 9.0.4
    GIT_SHALLOW ON
    PATCH_COMMAND ${CMAKE_COMMAND} -P ${CMAKE_CURRENT_SOURCE_DIR}/hosts/juce-patches/apply.cmake)

# Oscairn Sample Packs and waveforms (github.com/oscairn/samplepack, MIT): the one reader of .osp
# and .owf files for the host and the Sampler, in Rust behind a C interface, at a pinned commit.
# Its CMake module builds the static library with cargo, offline, at the Rust release its
# rust-toolchain.toml pins; a local checkout can stand in with FETCHCONTENT_SOURCE_DIR_SAMPLEPACK.
set(OSCAIRN_SAMPLEPACK_COMMIT c87aa70d2aec4445e70140172e7dfad038ae31d0)
set(OSCAIRN_RUST_VERSION 1.95.0)
FetchContent_Declare(samplepack GIT_REPOSITORY https://github.com/oscairn/samplepack.git GIT_TAG ${OSCAIRN_SAMPLEPACK_COMMIT})
FetchContent_MakeAvailable(samplepack)
find_program(SAMPLEPACK_CARGO cargo HINTS "$ENV{CARGO_HOME}/bin" "$ENV{HOME}/.cargo/bin")
set(rust_help "install rustup (https://rustup.rs) and run: rustup toolchain install ${OSCAIRN_RUST_VERSION} --profile minimal --component clippy,rustfmt --target aarch64-unknown-linux-gnu (or pass -DSAMPLEPACK_CARGO=<cargo ${OSCAIRN_RUST_VERSION}>)")
if(NOT SAMPLEPACK_CARGO)
    message(FATAL_ERROR "Oscairn needs Rust ${OSCAIRN_RUST_VERSION} (cargo) to build samplepack: ${rust_help}")
endif()
# In samplepack's folder, so rustup takes its pinned release; never installing one at configure time.
execute_process(COMMAND ${CMAKE_COMMAND} -E env RUSTUP_AUTO_INSTALL=0 ${SAMPLEPACK_CARGO} --version
    WORKING_DIRECTORY "${samplepack_SOURCE_DIR}" RESULT_VARIABLE rust_result OUTPUT_VARIABLE rust_found ERROR_VARIABLE rust_error OUTPUT_STRIP_TRAILING_WHITESPACE)
if(NOT rust_result EQUAL 0 OR NOT rust_found MATCHES "^cargo ${OSCAIRN_RUST_VERSION}[ ]")
    message(FATAL_ERROR "Oscairn needs Rust ${OSCAIRN_RUST_VERSION} to build samplepack; ${SAMPLEPACK_CARGO} gives \"${rust_found}${rust_error}\": ${rust_help}")
endif()
list(APPEND CMAKE_MODULE_PATH "${samplepack_SOURCE_DIR}/cmake")
include(SamplePack) # samplepack::samplepack
set_property(TARGET samplepack::samplepack APPEND PROPERTY INTERFACE_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/core/plugin-host/src/rust_eh_personality.c")

add_subdirectory(core)
add_subdirectory(devices/roland-fantom0)
add_subdirectory(plugins)
add_subdirectory(hosts)
# The UI in Rust (docs/rust-ui.md), replacing the JUCE app's screens one by one. Off by default:
# cargo downloads its crates (Slint) on the first build.
option(OSCAIRN_UI "Build the Rust UI, oscairn-ui (downloads its crates)" OFF)
if(OSCAIRN_UI)
    add_subdirectory(ui)
    if(TARGET oscairn_standalone)
        add_dependencies(oscairn_standalone oscairn_ui) # the app carries it (hosts/standalone/CMakeLists.txt)
    endif()
endif()

# Every test runs with a home, a data folder and a settings folder of its own under the build tree, so no
# test reads or writes the real ones (~/Library/Application Support/Oscairn): HOME (and so JUCE's "~" and
# the engine's fallback) is one shared empty folder, build/test-home. The test-home-clear fixture empties
# it and build/test-data before the first test; test-home-empty runs after the last and fails when
# anything appeared in build/test-home. A test that names its own OSCAIRN_* folder keeps it.
set(oscairn_test_home "${CMAKE_BINARY_DIR}/test-home")
set(oscairn_test_data "${CMAKE_BINARY_DIR}/test-data")
# Rust's tools keep their toolchains and crate registry under HOME: point them at the real ones, resolved
# here (configure runs with the real HOME), offline and never installing a toolchain, so a test that
# runs cargo finds them and writes nothing into the fake home.
if(DEFINED ENV{CARGO_HOME} AND NOT "$ENV{CARGO_HOME}" STREQUAL "")
    set(oscairn_cargo_home "$ENV{CARGO_HOME}")
else()
    set(oscairn_cargo_home "$ENV{HOME}/.cargo")
endif()
if(DEFINED ENV{RUSTUP_HOME} AND NOT "$ENV{RUSTUP_HOME}" STREQUAL "")
    set(oscairn_rustup_home "$ENV{RUSTUP_HOME}")
else()
    set(oscairn_rustup_home "$ENV{HOME}/.rustup")
endif()
function(oscairn_isolate_tests dir)
    get_property(tests DIRECTORY "${dir}" PROPERTY TESTS)
    foreach(test IN LISTS tests)
        if(test MATCHES "^test-home-")
            continue()
        endif()
        get_property(environment TEST "${test}" DIRECTORY "${dir}" PROPERTY ENVIRONMENT)
        set(isolated "HOME=${oscairn_test_home}" "CARGO_HOME=${oscairn_cargo_home}" "RUSTUP_HOME=${oscairn_rustup_home}" "RUSTUP_AUTO_INSTALL=0" "CARGO_NET_OFFLINE=true")
        foreach(name IN ITEMS DATA CONFIG)
            if(NOT environment MATCHES "OSCAIRN_${name}_HOME=")
                string(TOLOWER "${name}" folder)
                list(APPEND isolated "OSCAIRN_${name}_HOME=${oscairn_test_data}/${test}/${folder}")
            endif()
        endforeach()
        set_property(TEST "${test}" DIRECTORY "${dir}" APPEND PROPERTY ENVIRONMENT ${isolated})
        set_property(TEST "${test}" DIRECTORY "${dir}" APPEND PROPERTY FIXTURES_REQUIRED test-home)
    endforeach()
    get_property(subdirectories DIRECTORY "${dir}" PROPERTY SUBDIRECTORIES)
    foreach(subdirectory IN LISTS subdirectories)
        oscairn_isolate_tests("${subdirectory}")
    endforeach()
endfunction()
add_test(NAME test-home-clear COMMAND ${CMAKE_COMMAND} -E rm -rf "${oscairn_test_home}" "${oscairn_test_data}")
add_test(NAME test-home-made COMMAND ${CMAKE_COMMAND} -E make_directory "${oscairn_test_home}")
set_tests_properties(test-home-clear PROPERTIES FIXTURES_SETUP test-home-cleared)
set_tests_properties(test-home-made PROPERTIES FIXTURES_REQUIRED test-home-cleared FIXTURES_SETUP test-home)
add_test(NAME test-home-empty COMMAND ${CMAKE_COMMAND} -DFOLDER=${oscairn_test_home} -P ${CMAKE_SOURCE_DIR}/tools/test-home-empty.cmake)
set_tests_properties(test-home-empty PROPERTIES FIXTURES_CLEANUP test-home)
oscairn_isolate_tests("${CMAKE_SOURCE_DIR}")
