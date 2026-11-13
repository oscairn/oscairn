# The Roland FANTOM-06, -07 and -08 (docs/fantom-0.md): its panel holder and reader
# (panel_reader.h), its message link's reader (BmcLink.h), its gpio keys' reader (InputKeys.h), the tool that maps the panel and the
# panel map. Built here for their tests; the engine links the readers (through hosts/audio's
# DeviceReaders.h), and the image installs the holder, the tool and the map, only with
# OSCAIRN_ROLAND_FANTOM0, so a product without this panel has none of it.
if(NOT UNIX)
    return()
endif()
find_package(Threads REQUIRED)

add_library(oscairn_panel STATIC src/panel_reader.cpp)
target_include_directories(oscairn_panel SYSTEM PRIVATE ${PROJECT_SOURCE_DIR}/core/control/third_party)
target_include_directories(oscairn_panel PRIVATE ${PROJECT_SOURCE_DIR}/core/control/src) # JsonInput.h
target_include_directories(oscairn_panel PUBLIC include)
target_link_libraries(oscairn_panel PUBLIC oscairn_control)
target_compile_options(oscairn_panel PRIVATE -Wall -Wextra)
add_executable(oscairn-panel-dump tools/panel_dump.cpp)
target_link_libraries(oscairn-panel-dump PRIVATE oscairn_panel)
target_compile_options(oscairn-panel-dump PRIVATE -Wall -Wextra)
add_executable(oscairn-panel-holder tools/panel_holder.cpp)
target_link_libraries(oscairn-panel-holder PRIVATE oscairn_panel)
target_compile_options(oscairn-panel-holder PRIVATE -Wall -Wextra)
add_executable(oscairn-panel-reader-test tests/panel_reader.cpp)
target_link_libraries(oscairn-panel-reader-test PRIVATE oscairn_panel)
target_compile_options(oscairn-panel-reader-test PRIVATE -Wall -Wextra)
add_test(NAME control-panel-reader COMMAND oscairn-panel-reader-test $<TARGET_FILE:kick-clap-fixture-normal>
    ${CMAKE_CURRENT_SOURCE_DIR}/panels/fantom-07.json $<TARGET_FILE:oscairn-panel-dump>)
set_tests_properties(control-panel-reader PROPERTIES TIMEOUT 60)

# The message link: its frame parser and its reader on a pty, plain POSIX.
add_executable(oscairn-bmc-link-test tests/bmc_link_test.cpp src/BmcLink.cpp)
target_include_directories(oscairn-bmc-link-test PRIVATE src)
target_link_libraries(oscairn-bmc-link-test PRIVATE Threads::Threads)
target_compile_options(oscairn-bmc-link-test PRIVATE -Wall -Wextra)
add_test(NAME engine-bmc-link COMMAND oscairn-bmc-link-test)
set_tests_properties(engine-bmc-link PROPERTIES TIMEOUT 30)

# oscairn-key-held's bit lookup, everywhere; the tool itself reads Linux input devices.
add_executable(oscairn-key-held-test tests/key_held_test.c)
add_test(NAME devices-key-held COMMAND oscairn-key-held-test)
# The bundle's start, stop and boot scripts, dry, in a stand-in bundle (they read /proc).
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    add_executable(oscairn-key-held tools/key_held.c)
    target_compile_options(oscairn-key-held PRIVATE -Wall -Wextra)
    # The scripts ask the engine for session.status with it (os/scripts/control-request.c).
    add_executable(oscairn-control-request ${PROJECT_SOURCE_DIR}/os/scripts/control-request.c)
    add_test(NAME devices-fantom-start COMMAND sh ${CMAKE_CURRENT_SOURCE_DIR}/tests/fantom_start.sh ${CMAKE_CURRENT_SOURCE_DIR}/bundle)
    set_tests_properties(devices-fantom-start PROPERTIES TIMEOUT 30)
    # WRITE, MENU and TEMPO (gpio-keys, evdev): input_event records through a pipe.
    add_executable(oscairn-input-keys-test tests/input_keys_test.cpp src/InputKeys.cpp)
    target_include_directories(oscairn-input-keys-test PRIVATE src)
    target_link_libraries(oscairn-input-keys-test PRIVATE oscairn_control Threads::Threads)
    target_compile_options(oscairn-input-keys-test PRIVATE -Wall -Wextra)
    add_test(NAME devices-input-keys COMMAND oscairn-input-keys-test)
    set_tests_properties(devices-input-keys PROPERTIES TIMEOUT 30)
endif()

if(OSCAIRN_ROLAND_FANTOM0)
    # The engine's panel and keys (hosts/audio's DeviceReaders.h).
    add_library(oscairn_roland_fantom0 STATIC src/DeviceReaders.cpp src/BmcLink.cpp)
    target_include_directories(oscairn_roland_fantom0 PRIVATE src ${PROJECT_SOURCE_DIR}/hosts/audio)
    target_link_libraries(oscairn_roland_fantom0 PUBLIC oscairn_panel Threads::Threads)
    target_compile_options(oscairn_roland_fantom0 PRIVATE -Wall -Wextra)
    if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
        target_sources(oscairn_roland_fantom0 PRIVATE src/InputKeys.cpp)
    endif()
    install(TARGETS oscairn-panel-dump oscairn-panel-holder RUNTIME DESTINATION bin)
    if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
        install(TARGETS oscairn-key-held oscairn-control-request RUNTIME DESTINATION bin)
    endif()
    install(FILES panels/fantom-07.json DESTINATION share/oscairn/panels)
endif()
