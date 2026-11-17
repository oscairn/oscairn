foreach(kind normal crash hang slow held_state process_crash process_hang note_crash hog leak)
    add_library(kick-clap-fixture-${kind} MODULE Fixture.cpp)
    target_include_directories(kick-clap-fixture-${kind} PRIVATE ../third_party/clap/include ../../clap-ext/include)
    target_compile_features(kick-clap-fixture-${kind} PRIVATE cxx_std_17)
    target_link_libraries(kick-clap-fixture-${kind} PRIVATE kick_clap_sanitizers)
    set_target_properties(kick-clap-fixture-${kind} PROPERTIES PREFIX "" SUFFIX ".clap")
    if(NOT kind STREQUAL "normal")
        target_compile_definitions(kick-clap-fixture-${kind} PRIVATE FIXTURE_${kind}=1)
    endif()
endforeach()
# The host's conformance with these fixtures: each plugin loaded as the engine loads one (the Rust host, through
# PluginHost), and discovery with its scan helper (kick-clap-scan, tools/clap-host).
add_executable(kick-clap-tests HostTests.cpp)
target_link_libraries(kick-clap-tests PRIVATE oscairn_plugin_host)
add_test(NAME kick-clap-host COMMAND kick-clap-tests
    $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:kick-clap-scan>
    $<TARGET_FILE:kick-clap-fixture-crash> $<TARGET_FILE:kick-clap-fixture-hang> $<TARGET_FILE:kick-clap-fixture-hog>)
set_tests_properties(kick-clap-host PROPERTIES TIMEOUT 30)
