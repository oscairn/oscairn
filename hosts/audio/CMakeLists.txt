set(OSCAIRN_AUDIO_BUFFER_FRAMES "128" CACHE STRING "Requested live audio buffer frames (0 uses device default)")
if(NOT OSCAIRN_AUDIO_BUFFER_FRAMES MATCHES "^(0|[1-9][0-9]*)$" OR OSCAIRN_AUDIO_BUFFER_FRAMES GREATER 1048576)
    message(FATAL_ERROR "OSCAIRN_AUDIO_BUFFER_FRAMES must be an integer from 0 to 1048576")
endif()

# Device and controller profiles (RFC 0024): the app reads them, and the engine its controllers'.
juce_add_binary_data(oscairn_standalone_profiles HEADER_NAME ProfileData.h NAMESPACE ProfileData SOURCES
    ../standalone/profiles/qemu-aarch64.json
    ../standalone/profiles/kick.json
    ../standalone/profiles/imx8mm-evk.json
    ../standalone/profiles/fantom-07.json
    ../standalone/profiles/modx-m.json
    ../standalone/profiles/ignite.json
    ../standalone/profiles/smk37-elite.json
    ../standalone/profiles/modx-m-usb.json)

add_library(oscairn_audio_host STATIC AudioEngine.cpp ControllerPorts.cpp ../standalone/Profile.cpp)
# HTTPS: JUCE's URL streams, except on macOS an ephemeral NSURLSession of its own, which keeps no cache on disk.
if(APPLE)
    target_sources(oscairn_audio_host PRIVATE WebTransport_mac.mm)
    set_source_files_properties(WebTransport_mac.mm PROPERTIES COMPILE_OPTIONS "-fobjc-arc")
    target_link_libraries(oscairn_audio_host PRIVATE "-framework Foundation")
else()
    target_sources(oscairn_audio_host PRIVATE WebTransport.cpp)
endif()
target_compile_definitions(oscairn_audio_host PUBLIC JUCE_WEB_BROWSER=0)
target_compile_definitions(oscairn_audio_host PRIVATE OSCAIRN_AUDIO_BUFFER_FRAMES=${OSCAIRN_AUDIO_BUFFER_FRAMES})
target_link_libraries(oscairn_audio_host PUBLIC oscairn_control oscairn_hardware juce::juce_audio_devices PRIVATE oscairn_standalone_profiles)
target_compile_options(oscairn_audio_host PRIVATE -Wall -Wextra)
# HTTPS on Linux through libcurl (JUCE_USE_CURL, on by default); macOS uses the system's. The apps
# that fetch from repositories say NEEDS_CURL, and JUCE builds the rest without it.
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    target_link_libraries(oscairn_audio_host PRIVATE juce::pkgconfig_JUCE_CURL_LINUX_DEPS)
    # ALSA raw MIDI input, where the kernel has no sequencer for JUCE's MIDI input (RawMidi.h).
    find_package(ALSA REQUIRED)
    target_sources(oscairn_audio_host PRIVATE RawMidi_linux.cpp)
    target_link_libraries(oscairn_audio_host PRIVATE ALSA::ALSA)
    # Its reader on pipes, with no sound card: reading, a device ending, devices coming and going.
    add_executable(oscairn-raw-midi-test raw_midi_test.cpp RawMidi_linux.cpp)
    target_link_libraries(oscairn-raw-midi-test PRIVATE ALSA::ALSA)
    target_compile_options(oscairn-raw-midi-test PRIVATE -Wall -Wextra)
    add_test(NAME engine-raw-midi COMMAND oscairn-raw-midi-test)
    set_tests_properties(engine-raw-midi PROPERTIES TIMEOUT 30)
endif()

# A control session on stdin and stdout that fetches from the web as the engine does, for
# installing from a repository served over HTTP (RFC 0031 step 2).
juce_add_console_app(oscairn_web_rpc_test PRODUCT_NAME "oscairn-web-rpc-test" NEEDS_CURL TRUE)
target_sources(oscairn_web_rpc_test PRIVATE web_rpc_test.cpp)
target_compile_definitions(oscairn_web_rpc_test PRIVATE JUCE_WEB_BROWSER=0)
target_link_libraries(oscairn_web_rpc_test PRIVATE oscairn_audio_host oscairn_control_json juce::juce_recommended_config_flags)
find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    add_test(NAME control-repository-web COMMAND "${Python3_EXECUTABLE}" "${PROJECT_SOURCE_DIR}/core/control/tests/repository.py" $<TARGET_FILE:oscairn-json-rpc-test> $<TARGET_FILE:oscairn_basic_synth>
        $<TARGET_FILE:oscairn-sign> $<TARGET_FILE:kick-clap-fixture-normal> "${PROJECT_SOURCE_DIR}/tools/package-repository/template/scripts/build-index.py" $<TARGET_FILE:oscairn_web_rpc_test>)
    set_tests_properties(control-repository-web PROPERTIES TIMEOUT 180)
    # Preinstalling as the engine does, with a web transport that must not be used (RFC 0031).
    add_test(NAME control-preinstall-web COMMAND "${Python3_EXECUTABLE}" "${PROJECT_SOURCE_DIR}/core/control/tests/preinstall.py" $<TARGET_FILE:oscairn-sign> $<TARGET_FILE:kick-clap-fixture-normal>
        "${PROJECT_SOURCE_DIR}/tools/package-repository/template/scripts/build-index.py" $<TARGET_FILE:oscairn_web_rpc_test>)
    set_tests_properties(control-preinstall-web PROPERTIES TIMEOUT 120)
endif()

# A FANTOM-0 panel and keys read in the engine (panel_reader.h, BmcLink.h), for images whose profile has them.
if(OSCAIRN_PANEL_ROLAND)
    target_compile_definitions(oscairn_audio_host PUBLIC OSCAIRN_PANEL_ROLAND=1)
    target_sources(oscairn_audio_host PRIVATE BmcLink.cpp)
    target_link_libraries(oscairn_audio_host PUBLIC oscairn_panel)
endif()

# The audio device chosen through the control API, on a fake device type: nothing reaches a speaker.
juce_add_console_app(oscairn_audio_devices_test PRODUCT_NAME "oscairn-audio-devices-test")
target_sources(oscairn_audio_devices_test PRIVATE device_selection_test.cpp)
target_compile_definitions(oscairn_audio_devices_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_MODAL_LOOPS_PERMITTED=1)
target_link_libraries(oscairn_audio_devices_test PRIVATE oscairn_audio_host oscairn_control_json juce::juce_recommended_config_flags)
add_dependencies(oscairn_audio_devices_test oscairn_step_sequencer oscairn_basic_synth oscairn_delay oscairn-plugin-worker)
add_test(NAME engine-audio-devices COMMAND oscairn_audio_devices_test $<TARGET_FILE_DIR:oscairn_step_sequencer>)
add_test(NAME engine-audio-devices-worker COMMAND oscairn_audio_devices_test $<TARGET_FILE_DIR:oscairn_step_sequencer> $<TARGET_FILE:oscairn-plugin-worker>)
set_tests_properties(engine-audio-devices engine-audio-devices-worker PROPERTIES TIMEOUT 60)

# External controllers kept by the engine (RFC 0024), on input names alone: no MIDI device is opened.
juce_add_console_app(oscairn_controller_ports_test PRODUCT_NAME "oscairn-controller-ports-test")
target_sources(oscairn_controller_ports_test PRIVATE controller_ports_test.cpp)
target_compile_definitions(oscairn_controller_ports_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0)
target_link_libraries(oscairn_controller_ports_test PRIVATE oscairn_audio_host oscairn_control_json juce::juce_recommended_config_flags)
add_test(NAME engine-controller-ports COMMAND oscairn_controller_ports_test)
