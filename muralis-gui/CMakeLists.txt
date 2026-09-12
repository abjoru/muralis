cmake_minimum_required(VERSION 3.16)

# The workspace manifest is the single place the muralis version is declared, so
# the GUI reads it rather than carrying one of its own — before project(), which
# is handed the version it finds.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/../Cargo.toml" MURALIS_WORKSPACE_MANIFEST)
string(REGEX MATCH "\nversion = \"([^\"]+)\"" _ "${MURALIS_WORKSPACE_MANIFEST}")
set(MURALIS_VERSION "${CMAKE_MATCH_1}")
if(NOT MURALIS_VERSION)
    message(FATAL_ERROR "no workspace version in Cargo.toml to build muralis-gui with")
endif()

project(muralis-gui VERSION "${MURALIS_VERSION}" LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 REQUIRED COMPONENTS Quick QuickControls2 QuickTest Network Test)

# The muralis User-Agent has one definition — assets/user-agent.tmpl — rendered
# here for C++ exactly as muralis-core renders it for the Rust crates, with the
# workspace version read above.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/../assets/user-agent.tmpl" MURALIS_UA_TEMPLATE)
string(STRIP "${MURALIS_UA_TEMPLATE}" MURALIS_UA_TEMPLATE)
string(REPLACE "{version}" "${MURALIS_VERSION}" MURALIS_USER_AGENT "${MURALIS_UA_TEMPLATE}")
configure_file(src/muralis_user_agent.h.in
    "${CMAKE_CURRENT_BINARY_DIR}/muralis_user_agent.h" @ONLY)
message(STATUS "muralis User-Agent: ${MURALIS_USER_AGENT}")

# Everything that is not QML, shared by the app and its C++ tests.
set(MURALIS_GUI_SOURCES
    src/processrunner.h
    src/processrunner.cpp
    src/useragent.h
    src/useragent.cpp
    src/muralisnetwork.h
    src/muralisnetwork.cpp
    src/thumbnailcache.h
    src/thumbnailcache.cpp
)

qt_add_executable(muralis-gui
    src/main.cpp
    ${MURALIS_GUI_SOURCES}
)
target_include_directories(muralis-gui PRIVATE "${CMAKE_CURRENT_BINARY_DIR}")

set_source_files_properties(qml/Theme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(muralis-gui
    URI MuralisGui
    VERSION 1.0
    NO_CACHEGEN
    QML_FILES
        qml/main.qml
        qml/Theme.qml
        qml/FilterBar.qml
        qml/CategoryBar.qml
        qml/Retrieval.js
        qml/SearchView.qml
        qml/ThumbnailCard.qml
        qml/PreviewDrawer.qml
        qml/StatusBar.qml
)

target_link_libraries(muralis-gui PRIVATE Qt6::Quick Qt6::QuickControls2 Qt6::Network)

install(TARGETS muralis-gui DESTINATION bin)

# Presentation logic lives in qml/Retrieval.js so it can be exercised headless.
enable_testing()

qt_add_executable(muralis-gui-tests tests/main.cpp)
target_compile_definitions(muralis-gui-tests PRIVATE
    QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/tests")
target_link_libraries(muralis-gui-tests PRIVATE Qt6::Quick Qt6::QuickTest)

add_test(NAME version-tracks-workspace COMMAND "${CMAKE_COMMAND}"
    "-DWORKSPACE_MANIFEST=${CMAKE_CURRENT_SOURCE_DIR}/../Cargo.toml"
    "-DDECLARED_VERSION=${PROJECT_VERSION}"
    -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/version_tracks_workspace.cmake")

add_test(NAME qml-logic COMMAND muralis-gui-tests)
set_tests_properties(qml-logic PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

# The fetch path is asserted against a local server, never a live site.
foreach(suite http thumbnailcache)
    qt_add_executable(muralis-gui-tst-${suite} tests/tst_${suite}.cpp tests/testserver.h
        ${MURALIS_GUI_SOURCES})
    target_include_directories(muralis-gui-tst-${suite} PRIVATE "${CMAKE_CURRENT_BINARY_DIR}")
    target_link_libraries(muralis-gui-tst-${suite} PRIVATE Qt6::Quick Qt6::Network Qt6::Test)
    add_test(NAME ${suite} COMMAND muralis-gui-tst-${suite})
    set_tests_properties(${suite} PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")
endforeach()
