add_library(oscairn_control STATIC src/session.cpp src/performance.cpp src/parts.cpp src/pads.cpp src/physical_controls.cpp src/controller_mapping.cpp src/controller_input.cpp src/control_surface.cpp src/undo.cpp src/library.cpp src/item_types.cpp src/setlist.cpp src/package.cpp src/libraries.cpp src/repository.cpp src/packages.cpp src/preinstall.cpp src/budget.cpp src/audition.cpp src/signing.cpp src/trust.cpp third_party/miniz/miniz.c third_party/monocypher/monocypher.c third_party/monocypher/monocypher-ed25519.c third_party/sqlite/sqlite3.c)
set_source_files_properties(third_party/miniz/miniz.c third_party/monocypher/monocypher.c third_party/monocypher/monocypher-ed25519.c third_party/sqlite/sqlite3.c PROPERTIES COMPILE_OPTIONS "-w") # vendored, unmodified
# The library's index (RFC 0023 step 5): no extensions loaded, double-quoted strings refused.
set_source_files_properties(third_party/sqlite/sqlite3.c PROPERTIES COMPILE_DEFINITIONS "SQLITE_OMIT_LOAD_EXTENSION;SQLITE_DQS=0")
target_include_directories(oscairn_control SYSTEM PRIVATE third_party)
target_include_directories(oscairn_control PUBLIC include)
target_compile_definitions(oscairn_control PRIVATE OSCAIRN_VERSION="${PROJECT_VERSION}") # what a package's requires.oscairn is checked against (RFC 0031)
# The release, with its pre-release label (the top-level CMakeLists.txt): what preinstalled.json records the packages that come with it under (RFC 0031).
target_compile_definitions(oscairn_control PRIVATE OSCAIRN_RELEASE="${OSCAIRN_RELEASE}")
target_link_libraries(oscairn_control PUBLIC oscairn_engine PRIVATE $<$<PLATFORM_ID:Linux>:m>)
target_compile_options(oscairn_control PRIVATE -Wall -Wextra)

# JSON stays off the render path and out of the typed Control interface.
add_library(oscairn_control_json STATIC src/json_rpc.cpp)
target_include_directories(oscairn_control_json SYSTEM PRIVATE third_party)
target_link_libraries(oscairn_control_json PUBLIC oscairn_control oscairn_hardware)
target_compile_options(oscairn_control_json PRIVATE -Wall -Wextra)

if(UNIX)
    add_library(oscairn_control_socket STATIC src/local_socket.cpp src/remote_control.cpp)
    target_include_directories(oscairn_control_socket SYSTEM PRIVATE third_party)
    target_link_libraries(oscairn_control_socket PUBLIC oscairn_control_json)
    target_compile_options(oscairn_control_socket PRIVATE -Wall -Wextra)
endif()

add_executable(oscairn-package-test tests/package.cpp)
target_include_directories(oscairn-package-test SYSTEM PRIVATE third_party)
target_link_libraries(oscairn-package-test PRIVATE oscairn_control)
add_test(NAME control-package COMMAND oscairn-package-test)

add_executable(oscairn-item-types-test tests/item_types.cpp)
target_link_libraries(oscairn-item-types-test PRIVATE oscairn_control)
target_compile_options(oscairn-item-types-test PRIVATE -Wall -Wextra)
add_test(NAME control-item-types COMMAND oscairn-item-types-test)

add_executable(oscairn-setlists-test tests/setlists.cpp)
target_link_libraries(oscairn-setlists-test PRIVATE oscairn_control)
target_compile_options(oscairn-setlists-test PRIVATE -Wall -Wextra)
add_test(NAME control-setlists COMMAND oscairn-setlists-test $<TARGET_FILE:kick-clap-fixture-normal>)
set_tests_properties(control-setlists PROPERTIES TIMEOUT 60)

add_executable(oscairn-installed-types-test tests/installed_types.cpp)
target_link_libraries(oscairn-installed-types-test PRIVATE oscairn_control)
target_compile_options(oscairn-installed-types-test PRIVATE -Wall -Wextra)
add_test(NAME control-installed-types COMMAND oscairn-installed-types-test)

if(UNIX) # extractors run in a process of their own on Linux and macOS (RFC 0023 step 4)
    add_executable(oscairn-extract-fixture tests/extract_fixture.cpp)
    add_executable(oscairn-extractors-test tests/extractors.cpp)
    target_link_libraries(oscairn-extractors-test PRIVATE oscairn_control)
    target_compile_options(oscairn-extractors-test PRIVATE -Wall -Wextra)
    add_test(NAME control-extractors COMMAND oscairn-extractors-test $<TARGET_FILE:oscairn-extract-fixture> $<TARGET_FILE:oscairn-plugin-scan>)
    set_tests_properties(control-extractors PROPERTIES TIMEOUT 120)
endif()

# RFC 0023 step 5's gate: 50,000 items in place.
add_executable(oscairn-library-scale-test tests/library_scale.cpp)
target_link_libraries(oscairn-library-scale-test PRIVATE oscairn_control)
target_compile_options(oscairn-library-scale-test PRIVATE -Wall -Wextra)
# A sanitizer slows 50,000 items past the timeout and the 50 ms check means nothing there: fewer items, no timing.
if(CMAKE_CXX_FLAGS MATCHES "-fsanitize")
    set(library_scale_args --sanitized)
endif()
add_test(NAME control-library-scale COMMAND oscairn-library-scale-test ${library_scale_args})
set_tests_properties(control-library-scale PROPERTIES TIMEOUT 300)

add_executable(oscairn-signing-test tests/signing.cpp)
target_link_libraries(oscairn-signing-test PRIVATE oscairn_control)
add_test(NAME control-signing COMMAND oscairn-signing-test)

add_executable(oscairn-session-test tests/session.cpp)
target_include_directories(oscairn-session-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-session-test PRIVATE oscairn_control)
target_compile_options(oscairn-session-test PRIVATE -Wall -Wextra)
add_test(NAME control-session COMMAND oscairn-session-test
    $<TARGET_FILE:oscairn_step_sequencer> $<TARGET_FILE:oscairn_basic_synth> $<TARGET_FILE:oscairn_delay>)
set_tests_properties(control-session PROPERTIES TIMEOUT 30)

# RFC 0045's timing report: one stalled block in a worker, and each part of it apart.
add_executable(oscairn-worker-timing-test tests/worker_timing.cpp)
target_link_libraries(oscairn-worker-timing-test PRIVATE oscairn_control_json oscairn_control)
add_test(NAME control-worker-timing COMMAND oscairn-worker-timing-test $<TARGET_FILE:oscairn-plugin-worker> $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_basic_synth>)
set_tests_properties(control-worker-timing PROPERTIES TIMEOUT 60)
# The same with RFC 0045's spin: the engine spins for each result, the lone workers for their next blocks.
add_test(NAME control-worker-timing-spin COMMAND oscairn-worker-timing-test $<TARGET_FILE:oscairn-plugin-worker> $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_basic_synth>)
set_tests_properties(control-worker-timing-spin PROPERTIES TIMEOUT 60 ENVIRONMENT OSCAIRN_WORKER_SPIN_US=300)
# A Performance of ST Final's shape in workers, timed (RFC 0045): a short run here; longer by hand to compare settings.
add_executable(oscairn-worker-bench tests/worker_bench.cpp)
target_link_libraries(oscairn-worker-bench PRIVATE oscairn_control)
add_test(NAME control-worker-bench COMMAND oscairn-worker-bench $<TARGET_FILE:oscairn-plugin-worker> $<TARGET_FILE:kick-clap-fixture-normal> 2)
set_tests_properties(control-worker-bench PROPERTIES TIMEOUT 60)

add_executable(oscairn-json-rpc-test tests/json_rpc.cpp)
target_include_directories(oscairn-json-rpc-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-json-rpc-test PRIVATE oscairn_control_json)
target_compile_options(oscairn-json-rpc-test PRIVATE -Wall -Wextra)
add_test(NAME control-json-audio COMMAND oscairn-json-rpc-test $<TARGET_FILE:oscairn_basic_synth>)
set_tests_properties(control-json-audio PROPERTIES TIMEOUT 30)
find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    add_test(NAME control-json-protocol COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/json_rpc.py"
        $<TARGET_FILE:oscairn-json-rpc-test> $<TARGET_FILE:oscairn_basic_synth> $<TARGET_FILE:oscairn_delay>)
    set_tests_properties(control-json-protocol PROPERTIES TIMEOUT 30)
endif()

add_library(oscairn-control-fixture MODULE tests/control_fixture.cpp)
target_include_directories(oscairn-control-fixture PRIVATE ../clap-host/third_party/clap/include)
set_target_properties(oscairn-control-fixture PROPERTIES PREFIX "" SUFFIX ".clap")
add_executable(oscairn-control-flow-test tests/control_flow.cpp)
target_include_directories(oscairn-control-flow-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-control-flow-test PRIVATE oscairn_control_json)
target_compile_options(oscairn-control-flow-test PRIVATE -Wall -Wextra)
add_test(NAME control-flow COMMAND oscairn-control-flow-test $<TARGET_FILE:oscairn-control-fixture>)
set_tests_properties(control-flow PROPERTIES TIMEOUT 30)

if(UNIX)
    add_executable(oscairn-local-socket-test tests/local_socket.cpp)
    target_include_directories(oscairn-local-socket-test PRIVATE ../clap-host/tests)
    target_link_libraries(oscairn-local-socket-test PRIVATE oscairn_control_socket)
    target_compile_options(oscairn-local-socket-test PRIVATE -Wall -Wextra)
    add_test(NAME control-local-stream COMMAND oscairn-local-socket-test $<TARGET_FILE:oscairn-control-fixture>)
    add_test(NAME control-local-path-policy COMMAND oscairn-local-socket-test $<TARGET_FILE:oscairn-control-fixture> --path-policy)
    add_test(NAME control-local-path COMMAND oscairn-local-socket-test $<TARGET_FILE:oscairn-control-fixture> --named)
    set_tests_properties(control-local-stream control-local-path-policy control-local-path PROPERTIES TIMEOUT 30)
    set_tests_properties(control-local-path PROPERTIES SKIP_RETURN_CODE 77)
endif()

if(UNIX)
    add_test(NAME control-prepared-instances COMMAND oscairn-local-socket-test $<TARGET_FILE:kick-clap-fixture-normal> --instances)
    set_tests_properties(control-prepared-instances PROPERTIES TIMEOUT 30)
    add_test(NAME control-native-sounds COMMAND oscairn-local-socket-test $<TARGET_FILE:oscairn_basic_synth> --sounds)
    set_tests_properties(control-native-sounds PROPERTIES TIMEOUT 30)
endif()

add_executable(oscairn-sound-management-test tests/sound_management.cpp)
target_link_libraries(oscairn-sound-management-test PRIVATE oscairn_control_json)
target_include_directories(oscairn-sound-management-test SYSTEM PRIVATE third_party)
add_test(NAME control-sound-management COMMAND oscairn-sound-management-test)
if(OSCAIRN_LV2 AND OSCAIRN_LV2_SOURCE_DIR)
    add_test(NAME control-sound-management-lv2 COMMAND oscairn-sound-management-test
        "${PROJECT_BINARY_DIR}/core/lv2-host/tests/upstream sampler.lv2" "${PROJECT_BINARY_DIR}/core/lv2-host/tests/test fixture.lv2")
    set_tests_properties(control-sound-management-lv2 PROPERTIES TIMEOUT 30)
endif()

if(UNIX AND OSCAIRN_LV2 AND OSCAIRN_LV2_SOURCE_DIR)
    add_test(NAME control-remote-properties COMMAND oscairn-local-socket-test
        "${PROJECT_BINARY_DIR}/core/lv2-host/tests/upstream sampler.lv2" --properties)
    set_tests_properties(control-remote-properties PROPERTIES TIMEOUT 30)
endif()

if(UNIX)
    add_test(NAME control-native-effects COMMAND oscairn-local-socket-test $<TARGET_FILE:oscairn_basic_synth> --effects $<TARGET_FILE:oscairn_delay>)
    set_tests_properties(control-native-effects PROPERTIES TIMEOUT 30)
    add_test(NAME control-remote-parts COMMAND oscairn-local-socket-test $<TARGET_FILE:oscairn_basic_synth> --parts $<TARGET_FILE:oscairn_delay>)
    set_tests_properties(control-remote-parts PROPERTIES TIMEOUT 30)
endif()

add_executable(oscairn-sign ../../tools/oscairn-sign/oscairn_sign.cpp)
target_link_libraries(oscairn-sign PRIVATE oscairn_control)
target_compile_options(oscairn-sign PRIVATE -Wall -Wextra)
add_executable(oscairn-archives-test tests/archives.cpp)
target_include_directories(oscairn-archives-test SYSTEM PRIVATE third_party)
target_link_libraries(oscairn-archives-test PRIVATE oscairn_control)
add_executable(oscairn-libraries-test tests/libraries.cpp)
target_link_libraries(oscairn-libraries-test PRIVATE oscairn_control)
add_test(NAME control-libraries COMMAND oscairn-libraries-test $<TARGET_FILE:oscairn-sign>
    $<TARGET_FILE:oscairn_basic_synth> $<TARGET_FILE:oscairn_delay> $<TARGET_FILE:oscairn_splitters> $<TARGET_FILE:oscairn_step_sequencer>
    $<TARGET_FILE:oscairn_transpose> $<TARGET_FILE:oscairn_midi_delay> $<TARGET_FILE:kick-clap-fixture-normal>
    "$<$<TARGET_EXISTS:oscairn_modular>:$<TARGET_FILE:oscairn_modular>>")
set_tests_properties(control-libraries PROPERTIES TIMEOUT 120)
if(BUILD_TESTING) # the Legacy Sampler
    add_test(NAME control-archives COMMAND oscairn-archives-test $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn-sign> $<TARGET_FILE:oscairn_splitters> $<TARGET_FILE:oscairn_sampler>)
endif()
if(Python3_Interpreter_FOUND)
    # The package repository template's index builder (RFC 0031), signed with oscairn-sign.
    add_test(NAME package-repository-index COMMAND "${Python3_EXECUTABLE}" "${PROJECT_SOURCE_DIR}/tools/package-repository/test_build_index.py" $<TARGET_FILE:oscairn-sign>)
    set_tests_properties(package-repository-index PROPERTIES TIMEOUT 120)
    # Its checks of revoked.json, at least as strict as a device's reading of the list.
    add_test(NAME package-repository-revocations COMMAND "${Python3_EXECUTABLE}" "${PROJECT_SOURCE_DIR}/tools/package-repository/template/scripts/test_revocations.py" $<TARGET_FILE:oscairn-sign>)
    set_tests_properties(package-repository-revocations PROPERTIES TIMEOUT 300)
    # Installing from a repository that index builder made (RFC 0031 step 1), through RPC.
    add_test(NAME control-repository COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/repository.py" $<TARGET_FILE:oscairn-json-rpc-test> $<TARGET_FILE:oscairn_basic_synth>
        $<TARGET_FILE:oscairn-sign> $<TARGET_FILE:kick-clap-fixture-normal> "${PROJECT_SOURCE_DIR}/tools/package-repository/template/scripts/build-index.py")
    set_tests_properties(control-repository PROPERTIES TIMEOUT 120)
    # The packages that come with a release, from a repository snapshot (RFC 0031).
    add_test(NAME control-preinstall COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/preinstall.py" $<TARGET_FILE:oscairn-sign> $<TARGET_FILE:kick-clap-fixture-normal>
        "${PROJECT_SOURCE_DIR}/tools/package-repository/template/scripts/build-index.py" $<TARGET_FILE:oscairn-json-rpc-test> $<TARGET_FILE:oscairn_basic_synth>)
    set_tests_properties(control-preinstall PROPERTIES TIMEOUT 120)
endif()
# For third-party plugins (tested here on two of ours): a plugin through the library, an archive and the store.
add_executable(oscairn-plugin-check tests/plugin_check.cpp)
target_link_libraries(oscairn-plugin-check PRIVATE oscairn_control)
# A note effect is checked for the notes it sends, an audio effect for sound.
add_test(NAME control-plugin-check-note-effect COMMAND oscairn-plugin-check $<TARGET_FILE:oscairn_transpose> org.oscairn.transpose effect)
# RFC 0025's CPU estimates: a stall or a burst does not spoil them, and a spoiled one can be forgotten.
add_executable(oscairn-estimates-test tests/estimates.cpp)
target_link_libraries(oscairn-estimates-test PRIVATE oscairn_control_json oscairn_control)
add_test(NAME control-estimates COMMAND oscairn-estimates-test $<TARGET_FILE:oscairn-plugin-worker> $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn-voice-fixture>)
set_tests_properties(control-estimates PROPERTIES TIMEOUT 120)
if(CMAKE_CXX_FLAGS MATCHES "-fsanitize")
    target_compile_definitions(oscairn-estimates-test PRIVATE OSCAIRN_SANITIZED=1)
    set_tests_properties(control-estimates PROPERTIES TIMEOUT 600)
endif()

# Given relative to the working folder, as a packager types it.
add_test(NAME control-plugin-check-audio-effect COMMAND oscairn-plugin-check ./$<TARGET_FILE_NAME:oscairn_delay> org.oscairn.delay effect
    WORKING_DIRECTORY $<TARGET_FILE_DIR:oscairn_delay>)
set_tests_properties(control-plugin-check-note-effect control-plugin-check-audio-effect PROPERTIES TIMEOUT 60)

add_executable(oscairn-parts-test tests/parts.cpp)
target_include_directories(oscairn-parts-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-parts-test PRIVATE oscairn_control)
add_test(NAME control-parts COMMAND oscairn-parts-test
    $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn-audio-fixture> $<TARGET_FILE:oscairn_delay> $<TARGET_FILE:oscairn_splitters> $<TARGET_FILE:oscairn_transpose> $<TARGET_FILE:oscairn-follower-fixture> $<TARGET_FILE:oscairn-voice-fixture>)
set_tests_properties(control-parts PROPERTIES TIMEOUT 60)
if(CMAKE_CXX_FLAGS MATCHES "-fsanitize")
    set_tests_properties(control-parts PROPERTIES TIMEOUT 600) # 330 s under a local ThreadSanitizer build
endif()

# RFC 0011's controls driven by plugin lanes, flattened into the graph; again with every plugin in a worker.
add_executable(oscairn-control-source-test tests/control_source.cpp)
target_link_libraries(oscairn-control-source-test PRIVATE oscairn_control)
add_test(NAME control-source COMMAND oscairn-control-source-test $<TARGET_FILE:oscairn_step_sequencer> $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_lfo> $<TARGET_FILE:oscairn_basic_synth>)
add_test(NAME control-source-workers COMMAND oscairn-control-source-test $<TARGET_FILE:oscairn_step_sequencer> $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_lfo> $<TARGET_FILE:oscairn_basic_synth>)
set_tests_properties(control-source control-source-workers PROPERTIES TIMEOUT 120)
set_tests_properties(control-source-workers PROPERTIES ENVIRONMENT "OSCAIRN_PLUGIN_WORKER=$<TARGET_FILE:oscairn-plugin-worker>;OSCAIRN_PLUGIN_SCAN=$<TARGET_FILE:oscairn-plugin-scan>;OSCAIRN_WORKER_BUDGET_US=50000")

add_executable(oscairn-controller-input-test tests/controller_input.cpp)
target_include_directories(oscairn-controller-input-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-controller-input-test PRIVATE oscairn_control_socket)
add_test(NAME control-controller-input COMMAND oscairn-controller-input-test $<TARGET_FILE:kick-clap-fixture-normal>)
set_tests_properties(control-controller-input PROPERTIES TIMEOUT 60 RUN_SERIAL TRUE) # measures latency: alone, and judged on the best of three runs (see the test)

add_executable(oscairn-physical-controls-test tests/physical_controls.cpp)
target_link_libraries(oscairn-physical-controls-test PRIVATE oscairn_control)
add_test(NAME control-physical-controls COMMAND oscairn-physical-controls-test $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_delay> $<TARGET_FILE:oscairn_basic_synth>)
set_tests_properties(control-physical-controls PROPERTIES TIMEOUT 60)

add_executable(oscairn-control-surface-test tests/control_surface.cpp)
target_link_libraries(oscairn-control-surface-test PRIVATE oscairn_control)
add_test(NAME control-surface COMMAND oscairn-control-surface-test)

add_executable(oscairn-sounds-test tests/sounds.cpp)
target_include_directories(oscairn-sounds-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-sounds-test PRIVATE oscairn_control)
add_test(NAME control-sounds COMMAND oscairn-sounds-test $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_delay>)
set_tests_properties(control-sounds PROPERTIES TIMEOUT 60)

add_executable(oscairn-import-test tests/import.cpp)
target_include_directories(oscairn-import-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-import-test PRIVATE oscairn_control)
add_test(NAME control-import COMMAND oscairn-import-test $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_delay>)
set_tests_properties(control-import PROPERTIES TIMEOUT 60)

add_executable(oscairn-plugin-failures-test tests/plugin_failures.cpp)
target_link_libraries(oscairn-plugin-failures-test PRIVATE oscairn_control_json)
add_test(NAME control-plugin-failures COMMAND oscairn-plugin-failures-test $<TARGET_FILE:oscairn-plugin-worker> $<TARGET_FILE:kick-clap-fixture-process_crash> $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn-plugin-scan>)
set_tests_properties(control-plugin-failures PROPERTIES TIMEOUT 120) # a hung preset provider takes the scan helper's 15 s

# The Basic Synth holding and declaring 16 MiB a sound, as the Oscairn Analog does (RFC 0025).
add_library(oscairn-growing-synth MODULE tests/growing_synth.cpp)
target_link_libraries(oscairn-growing-synth PRIVATE oscairn_clap_headers)
set_target_properties(oscairn-growing-synth PROPERTIES PREFIX "" SUFFIX ".clap" CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)
add_executable(oscairn-worker-sound-memory-test tests/worker_sound_memory.cpp)
target_link_libraries(oscairn-worker-sound-memory-test PRIVATE oscairn_control)
add_test(NAME control-worker-sound-memory COMMAND oscairn-worker-sound-memory-test $<TARGET_FILE:oscairn-plugin-worker> $<TARGET_FILE:oscairn-growing-synth>)
set_tests_properties(control-worker-sound-memory PROPERTIES TIMEOUT 60)

add_executable(oscairn-undo-test tests/undo.cpp)
target_include_directories(oscairn-undo-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-undo-test PRIVATE oscairn_control)
add_test(NAME control-undo COMMAND oscairn-undo-test $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_splitters>)
set_tests_properties(control-undo PROPERTIES TIMEOUT 60)

add_executable(oscairn-pads-test tests/pads.cpp)
target_include_directories(oscairn-pads-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-pads-test PRIVATE oscairn_control)
add_test(NAME control-pads COMMAND oscairn-pads-test $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_basic_synth>)

add_executable(oscairn-performance-test tests/performance.cpp)
target_include_directories(oscairn-performance-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-performance-test PRIVATE oscairn_control)
add_test(NAME control-performance COMMAND oscairn-performance-test
    $<TARGET_FILE:oscairn_basic_synth> $<TARGET_FILE:oscairn_delay> $<TARGET_FILE:oscairn_step_sequencer> $<TARGET_FILE:kick-clap-fixture-normal>)
set_tests_properties(control-performance PROPERTIES TIMEOUT 60)
if(CMAKE_CXX_FLAGS MATCHES "-fsanitize")
    set_tests_properties(control-performance PROPERTIES TIMEOUT 600) # 58 to 59 s in CI's ThreadSanitizer job
endif()
# The Performance-switch tests load and render many instances; a sanitizer slows that several
# times over (control-performance-switch-workers took 183 s in a local ThreadSanitizer build).
set(switch_timeout 120)
if(CMAKE_CXX_FLAGS MATCHES "-fsanitize")
    set(switch_timeout 600)
endif()
if(BUILD_TESTING) # the Legacy Sampler
    add_executable(oscairn-setlist-change-test tests/setlist_change.cpp)
    target_include_directories(oscairn-setlist-change-test PRIVATE ../clap-host/tests)
    target_link_libraries(oscairn-setlist-change-test PRIVATE oscairn_control)
    add_test(NAME control-setlist-change COMMAND oscairn-setlist-change-test
        $<TARGET_FILE:oscairn_basic_synth> $<TARGET_FILE:oscairn_sampler> $<TARGET_FILE:oscairn_delay>)
    set_tests_properties(control-setlist-change PROPERTIES TIMEOUT ${switch_timeout})
    add_executable(oscairn-performance-switch-test tests/performance_switch.cpp)
    target_include_directories(oscairn-performance-switch-test PRIVATE ../clap-host/tests)
    target_link_libraries(oscairn-performance-switch-test PRIVATE oscairn_control)
    add_test(NAME control-performance-switch COMMAND oscairn-performance-switch-test
        $<TARGET_FILE:oscairn_basic_synth> $<TARGET_FILE:oscairn_sampler> $<TARGET_FILE:oscairn_delay> $<TARGET_FILE:kick-clap-fixture-normal>)
    set_tests_properties(control-performance-switch PROPERTIES TIMEOUT ${switch_timeout})
endif()
if(Python3_Interpreter_FOUND)
    add_test(NAME control-performance-format COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/performance.py"
        $<TARGET_FILE:oscairn-json-rpc-test> $<TARGET_FILE:oscairn_basic_synth>)
    set_tests_properties(control-performance-format PROPERTIES TIMEOUT 30)
    add_test(NAME control-library COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/library.py"
        $<TARGET_FILE:oscairn-json-rpc-test> $<TARGET_FILE:oscairn_basic_synth>)
    set_tests_properties(control-library PROPERTIES TIMEOUT 30)
endif()

add_executable(oscairn-performance-switch-stress-test tests/performance_switch_stress.cpp)
target_link_libraries(oscairn-performance-switch-stress-test PRIVATE oscairn_control)
target_compile_options(oscairn-performance-switch-stress-test PRIVATE -Wall -Wextra)
add_test(NAME control-performance-switch-stress COMMAND oscairn-performance-switch-stress-test
    $<TARGET_FILE:oscairn_basic_synth> $<TARGET_FILE:oscairn_delay> $<TARGET_FILE:kick-clap-fixture-normal>)
set_tests_properties(control-performance-switch-stress PROPERTIES TIMEOUT ${switch_timeout})

add_executable(oscairn-hostile-notes-test tests/hostile_notes.cpp)
target_link_libraries(oscairn-hostile-notes-test PRIVATE oscairn_control)
target_compile_options(oscairn-hostile-notes-test PRIVATE -Wall -Wextra)
add_test(NAME control-hostile-notes COMMAND oscairn-hostile-notes-test
    $<TARGET_FILE:oscairn-hostile-notes-fixture> $<TARGET_FILE:kick-clap-fixture-normal> $<TARGET_FILE:oscairn_basic_synth>)
set_tests_properties(control-hostile-notes PROPERTIES TIMEOUT ${switch_timeout})

add_executable(oscairn-shared-parts-test tests/shared_parts.cpp)
target_include_directories(oscairn-shared-parts-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-shared-parts-test PRIVATE oscairn_control_json)
add_test(NAME control-shared-parts COMMAND oscairn-shared-parts-test $<TARGET_FILE:oscairn_basic_synth>)
set_tests_properties(control-shared-parts PROPERTIES TIMEOUT ${switch_timeout})

add_executable(oscairn-sound-presets-test tests/sound_presets.cpp)
target_link_libraries(oscairn-sound-presets-test PRIVATE oscairn_control_json)
add_test(NAME control-sound-presets COMMAND oscairn-sound-presets-test $<TARGET_FILE:oscairn_basic_synth>)
# Plays an installed plugin offline, for tools/package-repository/live-check.py; no test, as that needs the network.
add_executable(oscairn-installed-play tests/installed_play.cpp)
target_link_libraries(oscairn-installed-play PRIVATE oscairn_control_json)
target_compile_options(oscairn-installed-play PRIVATE -Wall -Wextra)

# Sessions, native sounds and effects and Performances again with every CLAP plugin in a worker (RFC 0010 step 5).
set(control_worker_tests
    "control-sound-presets|oscairn-sound-presets-test|$<TARGET_FILE:oscairn_basic_synth>"
    "control-shared-parts|oscairn-shared-parts-test|$<TARGET_FILE:oscairn_basic_synth>"
    "control-json-audio|oscairn-json-rpc-test|$<TARGET_FILE:oscairn_basic_synth>"
    "control-hostile-notes|oscairn-hostile-notes-test|$<TARGET_FILE:oscairn-hostile-notes-fixture>|$<TARGET_FILE:kick-clap-fixture-normal>|$<TARGET_FILE:oscairn_basic_synth>"
    "control-performance-switch-stress|oscairn-performance-switch-stress-test|$<TARGET_FILE:oscairn_basic_synth>|$<TARGET_FILE:oscairn_delay>|$<TARGET_FILE:kick-clap-fixture-normal>"
    "control-performance|oscairn-performance-test|$<TARGET_FILE:oscairn_basic_synth>|$<TARGET_FILE:oscairn_delay>|$<TARGET_FILE:oscairn_step_sequencer>|$<TARGET_FILE:kick-clap-fixture-normal>")
if(BUILD_TESTING) # the Legacy Sampler
    list(APPEND control_worker_tests
        "control-performance-switch|oscairn-performance-switch-test|$<TARGET_FILE:oscairn_basic_synth>|$<TARGET_FILE:oscairn_sampler>|$<TARGET_FILE:oscairn_delay>|$<TARGET_FILE:kick-clap-fixture-normal>")
endif()
if(UNIX)
    list(APPEND control_worker_tests
        "control-native-sounds|oscairn-local-socket-test|$<TARGET_FILE:oscairn_basic_synth>|--sounds"
        "control-native-effects|oscairn-local-socket-test|$<TARGET_FILE:oscairn_basic_synth>|--effects|$<TARGET_FILE:oscairn_delay>")
endif()
foreach(entry IN LISTS control_worker_tests)
    string(REPLACE "|" ";" parts "${entry}")
    list(POP_FRONT parts name)
    add_test(NAME ${name}-workers COMMAND ${parts})
    set_tests_properties(${name}-workers PROPERTIES TIMEOUT ${switch_timeout} ENVIRONMENT "OSCAIRN_PLUGIN_WORKER=$<TARGET_FILE:oscairn-plugin-worker>;OSCAIRN_PLUGIN_SCAN=$<TARGET_FILE:oscairn-plugin-scan>;OSCAIRN_WORKER_BUDGET_US=50000")
endforeach()

add_executable(oscairn-recording-test tests/recording.cpp)
target_include_directories(oscairn-recording-test PRIVATE ../clap-host/tests)
target_link_libraries(oscairn-recording-test PRIVATE oscairn_control_json)
target_compile_options(oscairn-recording-test PRIVATE -Wall -Wextra)
add_test(NAME control-recording COMMAND oscairn-recording-test $<TARGET_FILE:oscairn_basic_synth>)
set_tests_properties(control-recording PROPERTIES TIMEOUT 60)
