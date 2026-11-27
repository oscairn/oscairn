# The DAW plugin's core (RFC 0047 step 1): one Performance per instance, under every plugin format.
add_library(oscairn_plugin_core STATIC plugin_core.cpp)
target_include_directories(oscairn_plugin_core PUBLIC include)
target_link_libraries(oscairn_plugin_core PRIVATE oscairn_control)
target_compile_options(oscairn_plugin_core PRIVATE -Wall -Wextra)

add_executable(oscairn-plugin-core-test tests/plugin_core.cpp)
target_include_directories(oscairn-plugin-core-test PRIVATE ${PROJECT_SOURCE_DIR}/core/clap-host/tests) # AllocationGuard.h
target_link_libraries(oscairn-plugin-core-test PRIVATE oscairn_plugin_core oscairn_control)
target_compile_options(oscairn-plugin-core-test PRIVATE -Wall -Wextra)
add_test(NAME plugin-core COMMAND oscairn-plugin-core-test $<TARGET_FILE:oscairn_basic_synth> $<TARGET_FILE:oscairn_delay>)
# Every plugin in a worker process, as on the device, through the core's own setting.
add_test(NAME plugin-core-workers COMMAND oscairn-plugin-core-test $<TARGET_FILE:oscairn_basic_synth> $<TARGET_FILE:oscairn_delay> $<TARGET_FILE:oscairn-plugin-worker>)
set_tests_properties(plugin-core plugin-core-workers PROPERTIES TIMEOUT 120)
set_tests_properties(plugin-core-workers PROPERTIES ENVIRONMENT "OSCAIRN_PLUGIN_SCAN=$<TARGET_FILE:oscairn-plugin-scan>;OSCAIRN_WORKER_BUDGET_US=50000")
