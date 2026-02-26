cmake_minimum_required(VERSION 3.16)
project(muralis-gui VERSION 0.1.8 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 REQUIRED COMPONENTS Quick QuickControls2)

qt_add_executable(muralis-gui
    src/main.cpp
    src/processrunner.h
    src/processrunner.cpp
)

set_source_files_properties(qml/Theme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(muralis-gui
    URI MuralisGui
    VERSION 1.0
    QML_FILES
        qml/main.qml
        qml/Theme.qml
        qml/FilterBar.qml
        qml/SearchView.qml
        qml/ThumbnailCard.qml
        qml/PreviewDrawer.qml
        qml/StatusBar.qml
)

target_link_libraries(muralis-gui PRIVATE Qt6::Quick Qt6::QuickControls2)

install(TARGETS muralis-gui DESTINATION bin)
