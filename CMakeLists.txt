cmake_minimum_required(VERSION 3.28) # FetchContent_Declare EXCLUDE_FROM_ALL (plugins/), set_property(TEST ... DIRECTORY) below
project(oscairn VERSION 0.1.0 LANGUAGES C CXX)
# A release is named by this version and this label while there is one (docs/releasing.md):
# Oscairn-0.1.0-alpha.1-macos-arm64.dmg, tag v0.1.0-alpha.1. Empty for a final release.
set(OSCAIRN_PRERELEASE "alpha.1")
if(APPLE)
    enable_language(OBJCXX) # the CLAP host's plugin windows (core/clap-host/src/Window.mm), and JUCE
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
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
option(OSCAIRN_PANEL_ROLAND "Read a Roland FANTOM-0 panel's I2C messages in the engine (Linux)" OFF)
set(OSCAIRN_LV2_EXAMPLE_DIR "" CACHE PATH "Optional upstream LV2 example bundles for integration tests")
set(OSCAIRN_LV2_SOURCE_DIR "" CACHE PATH "Optional LV2 source checkout for unmodified eg-sampler Worker tests")

# Off skips the tests that use the deprecated Legacy Sampler, and the Legacy Sampler itself.
option(BUILD_TESTING "Build the tests that need the Legacy Sampler" ON)
enable_testing()

add_subdirectory(core)
add_subdirectory(plugins)
add_subdirectory(hosts)

# Every test gets a data folder of its own unless it names one, so no test writes into the real
# one (~/Library/Application Support/Oscairn): the engine keeps counters there.
function(oscairn_isolate_tests dir)
    get_property(tests DIRECTORY "${dir}" PROPERTY TESTS)
    foreach(test IN LISTS tests)
        get_property(environment TEST "${test}" DIRECTORY "${dir}" PROPERTY ENVIRONMENT)
        if(NOT environment MATCHES "OSCAIRN_DATA_HOME=")
            set_property(TEST "${test}" DIRECTORY "${dir}" APPEND PROPERTY ENVIRONMENT
                "OSCAIRN_DATA_HOME=${CMAKE_BINARY_DIR}/test-data/${test}")
        endif()
    endforeach()
    get_property(subdirectories DIRECTORY "${dir}" PROPERTY SUBDIRECTORIES)
    foreach(subdirectory IN LISTS subdirectories)
        oscairn_isolate_tests("${subdirectory}")
    endforeach()
endfunction()
oscairn_isolate_tests("${CMAKE_SOURCE_DIR}")
