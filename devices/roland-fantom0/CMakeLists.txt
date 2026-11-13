# The Roland FANTOM-06, -07 and -08 (docs/fantom-0.md): its panel holder and reader
# (panel_reader.h), its message link's reader (BmcLink.h), the tool that maps the panel and the
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

if(OSCAIRN_ROLAND_FANTOM0)
    # The engine's panel and keys (hosts/audio's DeviceReaders.h).
    add_library(oscairn_roland_fantom0 STATIC src/DeviceReaders.cpp src/BmcLink.cpp)
    target_include_directories(oscairn_roland_fantom0 PRIVATE src ${PROJECT_SOURCE_DIR}/hosts/audio)
    target_link_libraries(oscairn_roland_fantom0 PUBLIC oscairn_panel Threads::Threads)
    target_compile_options(oscairn_roland_fantom0 PRIVATE -Wall -Wextra)
    install(TARGETS oscairn-panel-dump oscairn-panel-holder RUNTIME DESTINATION bin)
    install(FILES panels/fantom-07.json DESTINATION share/oscairn/panels)
endif()
