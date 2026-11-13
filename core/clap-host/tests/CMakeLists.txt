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
add_executable(kick-clap-tests HostTests.cpp)
target_link_libraries(kick-clap-tests PRIVATE kick_clap_host)
add_test(NAME kick-clap-host COMMAND kick-clap-tests
    $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:kick-clap-scan>
    $<TARGET_FILE:kick-clap-fixture-crash> $<TARGET_FILE:kick-clap-fixture-hang> $<TARGET_FILE:kick-clap-fixture-hog>)
set_tests_properties(kick-clap-host PROPERTIES TIMEOUT 30)
if(NOT WIN32)
    add_test(NAME kick-clap-scanner-cli COMMAND ${CMAKE_COMMAND} -E env
        "PATH=$<TARGET_FILE_DIR:kick-clap-scan>:$ENV{PATH}"
        kick-clap-scan $<TARGET_FILE:kick-clap-fixture-normal>)
    set_tests_properties(kick-clap-scanner-cli PROPERTIES TIMEOUT 10)
endif()

add_executable(kick-clap-benchmark-tests BenchmarkTests.cpp)
target_link_libraries(kick-clap-benchmark-tests PRIVATE kick_clap_benchmark)
add_test(NAME kick-clap-benchmark COMMAND kick-clap-benchmark-tests
    $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:kick-clap-fixture-slow> $<TARGET_FILE:kick-clap-fixture-held_state>)
set_tests_properties(kick-clap-benchmark PROPERTIES TIMEOUT 20)
find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    add_test(NAME kick-clap-benchmark-cli COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/BenchmarkCliTests.py"
        $<TARGET_FILE:kick-clap-bench> $<TARGET_FILE:kick-clap-fixture-normal>
        $<TARGET_FILE:kick-clap-fixture-process_crash> $<TARGET_FILE:kick-clap-fixture-process_hang>
        "${CMAKE_CURRENT_SOURCE_DIR}/../tools/benchmark_matrix.py")
    set_tests_properties(kick-clap-benchmark-cli PROPERTIES TIMEOUT 30)
endif()
