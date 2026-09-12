# The GUI declares no version of its own. Whatever CMake reports for it must be
# the workspace manifest's version, read here rather than taken on trust, so a
# literal reintroduced into the project() call fails this test.
file(READ "${WORKSPACE_MANIFEST}" manifest)
string(REGEX MATCH "\nversion = \"([^\"]+)\"" _ "${manifest}")
set(expected "${CMAKE_MATCH_1}")
if(NOT expected)
    message(FATAL_ERROR "no workspace version in ${WORKSPACE_MANIFEST}")
endif()
if(NOT DECLARED_VERSION STREQUAL expected)
    message(FATAL_ERROR
        "GUI project version is '${DECLARED_VERSION}', workspace version is '${expected}'")
endif()
