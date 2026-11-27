# The DAW plugin as CLAP (RFC 0047 step 2): Oscairn.clap, a shell over the plugin core. In the build tree only;
# nothing installs it into a plugin folder.
add_library(oscairn_clap MODULE clap_plugin.cpp)
target_link_libraries(oscairn_clap PRIVATE oscairn_plugin_core oscairn_clap_headers)
target_compile_options(oscairn_clap PRIVATE -Wall -Wextra)
set_target_properties(oscairn_clap PROPERTIES
    PREFIX "" SUFFIX ".clap" OUTPUT_NAME Oscairn
    CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)
if(APPLE)
    # A bundle: Oscairn.clap/Contents/{Info.plist,MacOS/Oscairn}. Only clap_entry is exported.
    set_target_properties(oscairn_clap PROPERTIES BUNDLE TRUE BUNDLE_EXTENSION clap SUFFIX ""
        MACOSX_BUNDLE_INFO_PLIST ${CMAKE_CURRENT_SOURCE_DIR}/Info.plist.in
        MACOSX_BUNDLE_BUNDLE_VERSION ${PROJECT_VERSION} MACOSX_BUNDLE_SHORT_VERSION_STRING ${PROJECT_VERSION})
    target_link_options(oscairn_clap PRIVATE -Wl,-exported_symbol,_clap_entry)
else()
    target_link_options(oscairn_clap PRIVATE -Wl,--exclude-libs,ALL) # the engine's symbols stay in the module
endif()

add_executable(oscairn-clap-shell-test tests/clap_shell.cpp)
target_include_directories(oscairn-clap-shell-test PRIVATE ${PROJECT_SOURCE_DIR}/core/clap-host/tests) # AllocationGuard.h
target_link_libraries(oscairn-clap-shell-test PRIVATE oscairn_control oscairn_clap_headers ${CMAKE_DL_LIBS})
target_compile_options(oscairn-clap-shell-test PRIVATE -Wall -Wextra)
add_dependencies(oscairn-clap-shell-test oscairn_clap oscairn_basic_synth)
add_test(NAME plugin-clap COMMAND oscairn-clap-shell-test $<TARGET_BUNDLE_DIR:oscairn_clap> $<TARGET_FILE:oscairn_basic_synth>)
set_tests_properties(plugin-clap PROPERTIES TIMEOUT 180)
