# The last test of a ctest run: the home the tests ran with (HOME) must still be empty, else a test
# wrote where the user's own files would be. Run as cmake -DFOLDER=<home> -P.
file(GLOB found "${FOLDER}/*" "${FOLDER}/.[!.]*")
if(found)
    list(JOIN found "\n  " list)
    message(FATAL_ERROR "tests wrote into the home folder ${FOLDER}:\n  ${list}")
endif()
