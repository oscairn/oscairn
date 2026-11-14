# JUCE's patch step (FetchContent_Declare(JUCE ... PATCH_COMMAND) in the top CMakeLists.txt), run in
# JUCE's source directory: applies every *.patch here, each once, so running it again on a patched
# tree changes nothing. A changed patch needs a fresh JUCE: delete _deps/juce-* in the build folder.
# Buildroot's JUCE (os/buildroot/package/oscairn-juce) applies them through links there: add one
# for a new patch.
file(GLOB patches "${CMAKE_CURRENT_LIST_DIR}/*.patch")
list(SORT patches)
foreach(patch IN LISTS patches)
    execute_process(COMMAND git apply --reverse --check "${patch}" RESULT_VARIABLE unapplied OUTPUT_QUIET ERROR_QUIET)
    if(unapplied)
        execute_process(COMMAND git apply "${patch}" RESULT_VARIABLE failed)
        if(failed)
            message(FATAL_ERROR "JUCE patch does not apply: ${patch}")
        endif()
    endif()
endforeach()
