juce_add_console_app(oscairn_engine_host PRODUCT_NAME "oscairn-engine" NEEDS_CURL TRUE)
set_target_properties(oscairn_engine_host PROPERTIES OUTPUT_NAME "oscairn-engine")
target_sources(oscairn_engine_host PRIVATE main.cpp)
target_compile_definitions(oscairn_engine_host PRIVATE
    JUCE_WEB_BROWSER=0 JUCE_MODAL_LOOPS_PERMITTED=1
    OSCAIRN_PLUGIN_DIR="$<TARGET_FILE_DIR:oscairn_step_sequencer>"
    OSCAIRN_PLUGIN_WORKER_PATH="$<TARGET_FILE:oscairn-plugin-worker>"
    OSCAIRN_PLUGIN_SCAN_PATH="$<TARGET_FILE:oscairn-plugin-scan>")
target_link_libraries(oscairn_engine_host PRIVATE oscairn_audio_host oscairn_host_process
    juce::juce_recommended_config_flags juce::juce_recommended_warning_flags)
add_dependencies(oscairn_engine_host oscairn_step_sequencer oscairn_basic_synth oscairn_delay oscairn-plugin-worker)
install(TARGETS oscairn_engine_host RUNTIME DESTINATION bin)

# The engine's audio timing over an interval, from its socket (tools/engine-timing): plain C++, no
# JUCE, installed with the engine.
add_executable(oscairn-engine-timing ${PROJECT_SOURCE_DIR}/tools/engine-timing/engine_timing.cpp)
target_include_directories(oscairn-engine-timing PRIVATE ${PROJECT_SOURCE_DIR}/core/plugin-host/include ${PROJECT_SOURCE_DIR}/core/control/src)
target_include_directories(oscairn-engine-timing SYSTEM PRIVATE ${PROJECT_SOURCE_DIR}/core/control/third_party)
target_compile_options(oscairn-engine-timing PRIVATE -Wall -Wextra)
install(TARGETS oscairn-engine-timing RUNTIME DESTINATION bin)
find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    add_test(NAME engine-timing COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/engine_timing.py"
        $<TARGET_FILE:oscairn_engine_host> $<TARGET_FILE:oscairn-engine-timing>)
    set_tests_properties(engine-timing PROPERTIES TIMEOUT 90)
    add_test(NAME engine-worker-given-up COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/worker_given_up.py"
        $<TARGET_FILE:oscairn_engine_host> $<TARGET_FILE:oscairn_basic_synth>)
    set_tests_properties(engine-worker-given-up PROPERTIES TIMEOUT 90)
endif()

# The control socket served as the engine serves it, without an audio device.
juce_add_console_app(oscairn_engine_pump_test PRODUCT_NAME "Oscairn Engine Pump Test")
target_sources(oscairn_engine_pump_test PRIVATE tests/pump.cpp)
target_compile_definitions(oscairn_engine_pump_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_MODAL_LOOPS_PERMITTED=1)
target_link_libraries(oscairn_engine_pump_test PRIVATE oscairn_audio_host oscairn_control_socket juce::juce_recommended_config_flags)
add_test(NAME engine-pump COMMAND oscairn_engine_pump_test)
set_tests_properties(engine-pump PROPERTIES TIMEOUT 30)

if(APPLE)
    # Console helpers need an embedded usage description for explicitly enabled capture.
    target_link_options(oscairn_engine_host PRIVATE "LINKER:-sectcreate,__TEXT,__info_plist,${CMAKE_CURRENT_SOURCE_DIR}/Info.plist")
    set_property(TARGET oscairn_engine_host APPEND PROPERTY LINK_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/Info.plist")
endif()
