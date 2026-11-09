# Desktop UI: managed helper by default; --in-process and --connect remain available.
juce_add_gui_app(oscairn_standalone
    PRODUCT_NAME "Oscairn"
    COMPANY_NAME "Oscairn"
    BUNDLE_ID org.oscairn.standalone
    NEEDS_CURL TRUE
    ICON_BIG "${CMAKE_CURRENT_SOURCE_DIR}/../../branding/app-icon-1024.png"
    ICON_SMALL "${CMAKE_CURRENT_SOURCE_DIR}/../../branding/app-icon-1024.png"
    MICROPHONE_PERMISSION_ENABLED TRUE
    MICROPHONE_PERMISSION_TEXT "Oscairn uses selected audio inputs for live instruments, effects and routing.")

juce_add_binary_data(oscairn_standalone_layouts SOURCES
    layouts/desktop.json
    layouts/kick.json
    layouts/kick-screen.json
    layouts/modx-m-screen.json
    layouts/ignite.json
    layouts/keys49.json
    layouts/compact.json)
# Device profiles (RFC 0024), apart from the layouts: some share their names.
juce_add_binary_data(oscairn_standalone_profiles HEADER_NAME ProfileData.h NAMESPACE ProfileData SOURCES
    profiles/qemu-aarch64.json
    profiles/kick.json
    profiles/imx8mm-evk.json
    profiles/fantom-07.json
    profiles/modx-m.json
    profiles/ignite.json
    profiles/smk37-elite.json
    profiles/modx-m-usb.json)

# Icons (Lucide, ISC: icons/LICENSE).
juce_add_binary_data(oscairn_standalone_icons HEADER_NAME IconData.h NAMESPACE IconData SOURCES
    icons/plus.svg
    icons/undo-2.svg
    icons/redo-2.svg
    icons/square-arrow-out-up-right.svg
    icons/list.svg
    icons/tags.svg
    icons/star.svg
    icons/chevron-down.svg
    icons/chevron-right.svg
    icons/check.svg
    icons/house.svg
    icons/arrow-left.svg
    icons/piano.svg
    icons/library-big.svg
    icons/list-music.svg
    icons/layout-grid.svg
    icons/octagon-x.svg
    icons/ellipsis-vertical.svg
    icons/layers.svg
    icons/circle.svg
    icons/history.svg
    icons/audio-lines.svg
    # Oscairn's mark (branding/README.md) and its alternate; Icons.h names the one shown.
    ../../branding/mark-on-dark.svg
    ../../branding/mark-on-light.svg
    ../../branding/mark-mono.svg
    ../../branding/mark-hills-on-dark.svg
    ../../branding/mark-hills-on-light.svg
    ../../branding/mark-hills-mono.svg)

# The Setlist app (RFC 0022 step 2): a program of its own, which the shell starts from the
# manifest beside it. Each app's folder (app.json and its build) is put together in Apps/ here,
# which the shell finds beside itself.
set(oscairn_apps "${CMAKE_CURRENT_BINARY_DIR}/Apps")
juce_add_gui_app(oscairn_setlist_app
    PRODUCT_NAME "Oscairn Setlist"
    COMPANY_NAME "Oscairn"
    BUNDLE_ID org.oscairn.setlist
    ICON_BIG "${CMAKE_CURRENT_SOURCE_DIR}/../../branding/app-icon-1024.png"
    ICON_SMALL "${CMAKE_CURRENT_SOURCE_DIR}/../../branding/app-icon-1024.png")
target_sources(oscairn_setlist_app PRIVATE apps/setlist/SetlistApp.cpp Setlist.cpp Engine.cpp Theme.cpp Splash.cpp SoundPickerModel.cpp)
target_compile_definitions(oscairn_setlist_app PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0)
target_link_libraries(oscairn_setlist_app PRIVATE oscairn_audio_host oscairn_host_process oscairn_control_socket oscairn_standalone_icons
    juce::juce_audio_utils juce::juce_gui_extra juce::juce_recommended_config_flags juce::juce_recommended_warning_flags)
if(APPLE)
    set(setlist_build "$<TARGET_BUNDLE_DIR:oscairn_setlist_app>")
    set(setlist_copy copy_directory "${setlist_build}" "${oscairn_apps}/oscairn.setlist/Oscairn Setlist.app")
else()
    set(setlist_copy copy_if_different "$<TARGET_FILE:oscairn_setlist_app>" "${oscairn_apps}/oscairn.setlist/")
endif()
add_custom_command(TARGET oscairn_setlist_app POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E make_directory "${oscairn_apps}/oscairn.setlist"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CMAKE_CURRENT_SOURCE_DIR}/apps/setlist/app.json" "${oscairn_apps}/oscairn.setlist/app.json"
    COMMAND ${CMAKE_COMMAND} -E ${setlist_copy}
    VERBATIM)
if(NOT APPLE)
    install(DIRECTORY "${oscairn_apps}/" DESTINATION share/oscairn/apps USE_SOURCE_PERMISSIONS)
endif()

target_sources(oscairn_standalone PRIVATE Main.cpp DesktopShell.cpp ParameterGrid.cpp CompactEditor.cpp ZoneEditor.cpp Engine.cpp Layout.cpp Panel.cpp Screen.cpp SoundPickerModel.cpp Theme.cpp PluginBrowser.cpp PadEditor.cpp PartsView.cpp PartsModel.cpp LibraryPane.cpp Catalog.cpp Setlist.cpp Profile.cpp Launcher.cpp PlayView.cpp Controllers.cpp Splash.cpp Recording.cpp Settings.cpp AppProcesses.cpp)
target_compile_definitions(oscairn_standalone PRIVATE
    JUCE_WEB_BROWSER=0
    JUCE_APPLICATION_NAME_STRING="$<TARGET_PROPERTY:oscairn_standalone,JUCE_PRODUCT_NAME>"
    JUCE_APPLICATION_VERSION_STRING="$<TARGET_PROPERTY:oscairn_standalone,JUCE_VERSION>"
    # The demo loads the plugins from the build tree unless --plugins says otherwise.
    OSCAIRN_PLUGIN_DIR="$<TARGET_FILE_DIR:oscairn_step_sequencer>")
target_link_libraries(oscairn_standalone PRIVATE
    oscairn_audio_host
    oscairn_host_process
    oscairn_control_socket
    oscairn_standalone_layouts
    oscairn_standalone_profiles
    oscairn_standalone_icons
    juce::juce_audio_utils
    juce::juce_gui_extra
    juce::juce_recommended_config_flags
    juce::juce_recommended_warning_flags)
set(bundled_plugins oscairn_step_sequencer oscairn_basic_synth oscairn_delay oscairn_lfo oscairn_transpose oscairn_midi_delay oscairn_splitters)
if(TARGET oscairn_modular)
    list(APPEND bundled_plugins oscairn_modular)
endif()
set(bundled_plugin_files "")
foreach(plugin IN LISTS bundled_plugins)
    list(APPEND bundled_plugin_files "$<TARGET_FILE:${plugin}>")
endforeach()
add_dependencies(oscairn_standalone oscairn-plugin-scan oscairn_engine_host oscairn-plugin-worker oscairn_setlist_app ${bundled_plugins})
set_property(TARGET oscairn_standalone APPEND PROPERTY LINK_DEPENDS
    "$<TARGET_FILE:oscairn-plugin-scan>;$<TARGET_FILE:oscairn_engine_host>;$<TARGET_FILE:oscairn-plugin-worker>;$<TARGET_FILE:oscairn_setlist_app>;${CMAKE_CURRENT_SOURCE_DIR}/apps/setlist/app.json;${bundled_plugin_files}")
add_custom_command(TARGET oscairn_standalone POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different $<TARGET_FILE:oscairn-plugin-scan> $<TARGET_FILE_DIR:oscairn_standalone>/oscairn-plugin-scan
    COMMAND ${CMAKE_COMMAND} -E copy_if_different $<TARGET_FILE:oscairn_engine_host> $<TARGET_FILE_DIR:oscairn_standalone>/oscairn-engine
    COMMAND ${CMAKE_COMMAND} -E copy_if_different $<TARGET_FILE:oscairn-plugin-worker> $<TARGET_FILE_DIR:oscairn_standalone>/oscairn-plugin-worker
    COMMAND ${CMAKE_COMMAND} -E make_directory $<TARGET_FILE_DIR:oscairn_standalone>/plugins
    COMMAND ${CMAKE_COMMAND} -E copy_directory ${CMAKE_CURRENT_SOURCE_DIR}/themes $<TARGET_FILE_DIR:oscairn_standalone>/themes
    COMMAND ${CMAKE_COMMAND} -E copy_directory ${oscairn_apps} $<TARGET_FILE_DIR:oscairn_standalone>/Apps
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        ${bundled_plugin_files}
        $<TARGET_FILE_DIR:oscairn_standalone>/plugins
    VERBATIM)

install(TARGETS oscairn_standalone RUNTIME DESTINATION bin BUNDLE DESTINATION Applications)
# Built-in themes (with their fonts) are ordinary theme folders; Linux finds them under share/.
if(NOT APPLE)
    install(DIRECTORY themes/ DESTINATION share/oscairn/themes)
endif()

juce_add_console_app(oscairn_theme_test PRODUCT_NAME "Oscairn Theme Test")
target_sources(oscairn_theme_test PRIVATE tests/theme.cpp Theme.cpp Splash.cpp)
target_link_libraries(oscairn_theme_test PRIVATE oscairn_standalone_icons juce::juce_gui_extra juce::juce_recommended_config_flags)
target_compile_definitions(oscairn_theme_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0)
add_test(NAME desktop-theme COMMAND oscairn_theme_test "${CMAKE_CURRENT_SOURCE_DIR}/themes" "${CMAKE_CURRENT_SOURCE_DIR}/theme-examples/midnight")

juce_add_console_app(oscairn_profile_test PRODUCT_NAME "Oscairn Profile Test")
target_sources(oscairn_profile_test PRIVATE tests/profile.cpp Profile.cpp)
target_link_libraries(oscairn_profile_test PRIVATE oscairn_standalone_profiles juce::juce_gui_extra juce::juce_recommended_config_flags)
target_compile_definitions(oscairn_profile_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0)
add_test(NAME desktop-profile COMMAND oscairn_profile_test)

# The pad editor against a session served on a socket, without an audio device.
juce_add_console_app(oscairn_pad_editor_test PRODUCT_NAME "Oscairn Pad Editor Test")
target_sources(oscairn_pad_editor_test PRIVATE tests/pad_editor.cpp PadEditor.cpp PartsView.cpp ParameterGrid.cpp ZoneEditor.cpp PartsModel.cpp Engine.cpp Theme.cpp SoundPickerModel.cpp)
target_link_libraries(oscairn_pad_editor_test PRIVATE oscairn_standalone_icons oscairn_audio_host oscairn_host_process oscairn_control_socket juce::juce_audio_utils juce::juce_gui_extra juce::juce_recommended_config_flags)
target_compile_definitions(oscairn_pad_editor_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_MODAL_LOOPS_PERMITTED=1)
add_dependencies(oscairn_pad_editor_test oscairn_step_sequencer oscairn_basic_synth oscairn_delay oscairn_splitters oscairn-plugin-scan oscairn-plugin-worker)
if(BUILD_TESTING) # the Legacy Sampler
    juce_add_console_app(oscairn_desktop_shell_test PRODUCT_NAME "Oscairn Desktop Shell Test")
    target_sources(oscairn_desktop_shell_test PRIVATE tests/desktop_shell.cpp DesktopShell.cpp PlayView.cpp Profile.cpp LibraryPane.cpp Catalog.cpp Settings.cpp Launcher.cpp Recording.cpp Setlist.cpp ParameterGrid.cpp CompactEditor.cpp ZoneEditor.cpp Screen.cpp Panel.cpp Layout.cpp PadEditor.cpp PartsView.cpp PartsModel.cpp Engine.cpp Theme.cpp SoundPickerModel.cpp)
    target_link_libraries(oscairn_desktop_shell_test PRIVATE oscairn_standalone_icons oscairn_standalone_profiles oscairn_audio_host oscairn_host_process oscairn_control_socket juce::juce_audio_utils juce::juce_gui_extra juce::juce_recommended_config_flags)
    target_compile_definitions(oscairn_desktop_shell_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_MODAL_LOOPS_PERMITTED=1)
    add_dependencies(oscairn_desktop_shell_test oscairn_step_sequencer oscairn_basic_synth oscairn_delay oscairn_transpose oscairn_sampler oscairn_splitters oscairn-plugin-scan kick-clap-fixture-normal oscairn-plugin-worker)
    add_test(NAME desktop-shell COMMAND oscairn_desktop_shell_test "$<TARGET_FILE_DIR:oscairn_step_sequencer>" $<TARGET_FILE:oscairn-plugin-scan> "${CMAKE_CURRENT_SOURCE_DIR}/themes")
    set_tests_properties(desktop-shell PROPERTIES TIMEOUT 120) # it covers most of the app; about 47 s alone
endif()
# A third-party plugin for RFC 0019's Compact gate, used where it is built (Odin 2 beside this
# checkout by default); the test skips it otherwise.
set(OSCAIRN_THIRD_PARTY_CLAP "${PROJECT_SOURCE_DIR}/../ports/odin2/build/Odin2_artefacts/Release/CLAP/Odin2.clap" CACHE FILEPATH "A third-party CLAP plugin for the desktop-shell test")
# The app runs every plugin in a worker, and lists their presets through the scan helper, so its
# tests do too (the budget allows for these threads' ordinary scheduling, as in the other worker tests).
set(worker_environment "OSCAIRN_PLUGIN_WORKER=$<TARGET_FILE:oscairn-plugin-worker>" "OSCAIRN_PLUGIN_SCAN=$<TARGET_FILE:oscairn-plugin-scan>" "OSCAIRN_WORKER_BUDGET_US=50000")
# The test fixture's stepped plugin, whose mode hides a control, for editors that follow it.
set(shell_environment "OSCAIRN_TEST_FIXTURE=$<TARGET_FILE:kick-clap-fixture-normal>" ${worker_environment})
if(EXISTS "${OSCAIRN_THIRD_PARTY_CLAP}")
    list(APPEND shell_environment "OSCAIRN_THIRD_PARTY_CLAP=${OSCAIRN_THIRD_PARTY_CLAP}")
endif()
if(BUILD_TESTING) # the Legacy Sampler
    set_tests_properties(desktop-shell PROPERTIES ENVIRONMENT "${shell_environment}")
endif()
# Desktop controls for features the control API had first, against a socket session; no MIDI device is opened.
juce_add_console_app(oscairn_desktop_gaps_test PRODUCT_NAME "Oscairn Desktop Gaps Test")
target_sources(oscairn_desktop_gaps_test PRIVATE tests/desktop_gaps.cpp DesktopShell.cpp PlayView.cpp Profile.cpp LibraryPane.cpp Catalog.cpp Settings.cpp Launcher.cpp Recording.cpp Setlist.cpp ParameterGrid.cpp CompactEditor.cpp ZoneEditor.cpp Screen.cpp Panel.cpp Layout.cpp PadEditor.cpp PartsView.cpp PartsModel.cpp Engine.cpp Theme.cpp SoundPickerModel.cpp)
target_link_libraries(oscairn_desktop_gaps_test PRIVATE oscairn_standalone_icons oscairn_standalone_profiles oscairn_audio_host oscairn_host_process oscairn_control_socket juce::juce_audio_utils juce::juce_gui_extra juce::juce_recommended_config_flags)
target_compile_definitions(oscairn_desktop_gaps_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_MODAL_LOOPS_PERMITTED=1)
# An instrument describing every oscairn.ui rich view (an FM operator view, envelopes, key maps), for the editors' tests, sounding a sine while a note is held; not installed.
add_library(oscairn_views_test_plugin MODULE tests/views_plugin.cpp)
target_link_libraries(oscairn_views_test_plugin PRIVATE oscairn_clap_headers)
set_target_properties(oscairn_views_test_plugin PROPERTIES PREFIX "" SUFFIX ".clap" OUTPUT_NAME oscairn-views-test CXX_VISIBILITY_PRESET hidden)
add_dependencies(oscairn_desktop_gaps_test oscairn_basic_synth oscairn-plugin-scan oscairn-plugin-worker oscairn_views_test_plugin)
add_test(NAME desktop-gaps COMMAND oscairn_desktop_gaps_test "$<TARGET_FILE_DIR:oscairn_step_sequencer>" $<TARGET_FILE:oscairn-plugin-scan> "${CMAKE_CURRENT_SOURCE_DIR}/themes")
set(gaps_environment "${worker_environment};OSCAIRN_TEST_VIEWS=$<TARGET_FILE:oscairn_views_test_plugin>")
# The Catalog's test reads a signed repository in a folder, made by the package repository template's index builder.
find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    add_dependencies(oscairn_desktop_gaps_test oscairn-sign kick-clap-fixture-normal)
    list(APPEND gaps_environment "OSCAIRN_TEST_MAKE_REPOSITORY=${Python3_EXECUTABLE}|${CMAKE_CURRENT_SOURCE_DIR}/tests/make_repository.py|$<TARGET_FILE:oscairn-sign>|$<TARGET_FILE:kick-clap-fixture-normal>|${PROJECT_SOURCE_DIR}/tools/package-repository/template/scripts/build-index.py")
endif()
set_tests_properties(desktop-gaps PROPERTIES TIMEOUT 120 ENVIRONMENT "${gaps_environment}")
# RFC 0022 step 2: the Setlist app as a process of its own, started and supervised from its manifest, against a socket session.
juce_add_console_app(oscairn_desktop_apps_test PRODUCT_NAME "Oscairn Desktop Apps Test")
target_sources(oscairn_desktop_apps_test PRIVATE tests/desktop_apps.cpp AppProcesses.cpp DesktopShell.cpp PlayView.cpp Profile.cpp LibraryPane.cpp Catalog.cpp Settings.cpp Launcher.cpp Recording.cpp Setlist.cpp ParameterGrid.cpp CompactEditor.cpp ZoneEditor.cpp Screen.cpp Panel.cpp Layout.cpp PadEditor.cpp PartsView.cpp PartsModel.cpp Engine.cpp Theme.cpp SoundPickerModel.cpp)
target_link_libraries(oscairn_desktop_apps_test PRIVATE oscairn_standalone_icons oscairn_standalone_profiles oscairn_audio_host oscairn_host_process oscairn_control_socket juce::juce_audio_utils juce::juce_gui_extra juce::juce_recommended_config_flags)
target_compile_definitions(oscairn_desktop_apps_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_MODAL_LOOPS_PERMITTED=1)
add_dependencies(oscairn_desktop_apps_test oscairn_setlist_app oscairn_step_sequencer oscairn_basic_synth oscairn-plugin-scan oscairn-plugin-worker)
add_test(NAME desktop-apps COMMAND oscairn_desktop_apps_test "$<TARGET_FILE_DIR:oscairn_step_sequencer>" "${oscairn_apps}" "${CMAKE_CURRENT_SOURCE_DIR}/themes")
set_tests_properties(desktop-apps PROPERTIES TIMEOUT 120 ENVIRONMENT "${worker_environment}")
# Phase 3b's "done when" end to end on the Panel layout, every plugin in a worker: with the in-repo
# plugins, and with the real engines from the folder OSCAIRN_PHASE3B_ENGINES names (skipped when unset).
juce_add_console_app(oscairn_phase3b_test PRODUCT_NAME "Oscairn Phase 3b Test")
target_sources(oscairn_phase3b_test PRIVATE tests/phase3b.cpp DesktopShell.cpp PlayView.cpp Profile.cpp LibraryPane.cpp Catalog.cpp Settings.cpp Launcher.cpp Recording.cpp Setlist.cpp ParameterGrid.cpp CompactEditor.cpp ZoneEditor.cpp Screen.cpp Panel.cpp Layout.cpp PadEditor.cpp PartsView.cpp PartsModel.cpp Engine.cpp Theme.cpp SoundPickerModel.cpp)
target_link_libraries(oscairn_phase3b_test PRIVATE oscairn_standalone_icons oscairn_standalone_profiles oscairn_audio_host oscairn_host_process oscairn_control_socket juce::juce_audio_utils juce::juce_gui_extra juce::juce_recommended_config_flags)
target_compile_definitions(oscairn_phase3b_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_MODAL_LOOPS_PERMITTED=1)
add_dependencies(oscairn_phase3b_test oscairn_basic_synth oscairn_delay oscairn_lfo oscairn-plugin-scan oscairn-plugin-worker oscairn_views_test_plugin)
add_test(NAME desktop-phase3b COMMAND oscairn_phase3b_test "$<TARGET_FILE_DIR:oscairn_step_sequencer>" $<TARGET_FILE:oscairn-plugin-scan> "${CMAKE_CURRENT_SOURCE_DIR}/themes")
add_test(NAME desktop-phase3b-engines COMMAND oscairn_phase3b_test "$<TARGET_FILE_DIR:oscairn_step_sequencer>" $<TARGET_FILE:oscairn-plugin-scan> "${CMAKE_CURRENT_SOURCE_DIR}/themes" --engines)
set_tests_properties(desktop-phase3b desktop-phase3b-engines PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77 ENVIRONMENT "${worker_environment};OSCAIRN_TEST_VIEWS=$<TARGET_FILE:oscairn_views_test_plugin>")
# The message thread never waits on the engine's socket for a mute, a fader drag, the views' polls or opening a Performance, which shows soon after the engine's load.
juce_add_console_app(oscairn_ui_waits_test PRODUCT_NAME "Oscairn UI Waits Test")
target_sources(oscairn_ui_waits_test PRIVATE tests/ui_waits.cpp PartsModel.cpp Engine.cpp SoundPickerModel.cpp)
target_link_libraries(oscairn_ui_waits_test PRIVATE oscairn_audio_host oscairn_host_process oscairn_control_socket juce::juce_audio_utils juce::juce_gui_extra juce::juce_recommended_config_flags)
target_compile_definitions(oscairn_ui_waits_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_MODAL_LOOPS_PERMITTED=1)
add_dependencies(oscairn_ui_waits_test oscairn_step_sequencer oscairn_basic_synth oscairn_delay oscairn-plugin-scan)
add_test(NAME ui-waits COMMAND oscairn_ui_waits_test "$<TARGET_FILE_DIR:oscairn_step_sequencer>" $<TARGET_FILE:oscairn-plugin-scan>)
set_tests_properties(ui-waits PROPERTIES TIMEOUT 120)
add_test(NAME desktop-pad-editor COMMAND oscairn_pad_editor_test "$<TARGET_FILE_DIR:oscairn_step_sequencer>" $<TARGET_FILE:oscairn-plugin-scan>)
# A crashing plugin in a Performance leaves the other parts playing and is reported (RFC 0010).
juce_add_console_app(oscairn_plugin_crash_test PRODUCT_NAME "Oscairn Plugin Crash Test")
target_sources(oscairn_plugin_crash_test PRIVATE tests/plugin_crash.cpp PadEditor.cpp PartsView.cpp ParameterGrid.cpp ZoneEditor.cpp PartsModel.cpp Engine.cpp Theme.cpp SoundPickerModel.cpp)
target_link_libraries(oscairn_plugin_crash_test PRIVATE oscairn_standalone_icons oscairn_audio_host oscairn_host_process oscairn_control_socket juce::juce_audio_utils juce::juce_gui_extra juce::juce_recommended_config_flags)
target_compile_definitions(oscairn_plugin_crash_test PRIVATE JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_MODAL_LOOPS_PERMITTED=1)
add_dependencies(oscairn_plugin_crash_test oscairn_basic_synth oscairn-plugin-scan oscairn-plugin-worker kick-clap-fixture-process_crash)
add_test(NAME desktop-plugin-crash COMMAND oscairn_plugin_crash_test "$<TARGET_FILE_DIR:oscairn_step_sequencer>" $<TARGET_FILE:oscairn-plugin-scan>
    $<TARGET_FILE:oscairn-plugin-worker> $<TARGET_FILE:kick-clap-fixture-process_crash>)
set_tests_properties(desktop-pad-editor desktop-plugin-crash PROPERTIES TIMEOUT 60 ENVIRONMENT "${worker_environment}")
