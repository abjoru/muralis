import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import MuralisGui

ApplicationWindow {
    id: window
    width: 1400
    height: 900
    visible: true
    title: "Muralis"

    Material.theme: Theme.isDark ? Material.Dark : Material.Light
    Material.accent: Theme.primary
    Material.primary: Theme.primary
    Material.background: Theme.surface
    Material.foreground: Theme.surfaceText

    color: Theme.surface

    // Keyboard mode state machine
    property string keyboardMode: "SEARCH"
    property var searchResults: []
    property var sourceList: []
    property int selectedIndex: -1
    property bool loading: false

    // Load sources on startup
    Component.onCompleted: {
        CLI.run("sources", ["sources", "list"])
        filterBar.focusSearch()
    }

    onKeyboardModeChanged: {
        if (keyboardMode === "SEARCH") {
            filterBar.focusSearch()
        } else {
            gridFocus.forceActiveFocus()
        }
    }

    Connections {
        target: CLI
        function onFinished(requestId, stdout, exitCode) {
            if (exitCode !== 0) {
                console.error("CLI failed:", requestId, stdout)
                loading = false
                return
            }

            if (requestId === "sources") {
                try {
                    sourceList = JSON.parse(stdout)
                } catch (e) {
                    console.error("Failed to parse sources:", e)
                }
            } else if (requestId === "search") {
                try {
                    var data = JSON.parse(stdout)
                    searchResults = data.results || []
                    searchView.hasMore = data.has_more || false
                } catch (e) {
                    console.error("Failed to parse search:", e)
                    searchResults = []
                }
                loading = false
            } else if (requestId.startsWith("fav-")) {
                var idx = parseInt(requestId.substring(4))
                if (idx >= 0 && idx < searchResults.length) {
                    var updated = searchResults.slice()
                    var item = Object.assign({}, updated[idx])
                    item.is_favorited = true
                    updated[idx] = item
                    searchResults = updated
                }
            }
        }
    }

    // Global keyboard handler for GRID/PREVIEW modes
    Item {
        id: gridFocus
        anchors.fill: parent

        Keys.onPressed: function(event) {
            if (keyboardMode === "GRID") {
                handleGridKeys(event)
            } else if (keyboardMode === "PREVIEW") {
                handlePreviewKeys(event)
            }
        }
    }

    function handleGridKeys(event) {
        var cols = searchView.columns
        if (event.key === Qt.Key_H || event.key === Qt.Key_Left) {
            if (selectedIndex > 0) selectedIndex--
            event.accepted = true
        } else if (event.key === Qt.Key_L || event.key === Qt.Key_Right) {
            if (selectedIndex < searchResults.length - 1) selectedIndex++
            event.accepted = true
        } else if (event.key === Qt.Key_J || event.key === Qt.Key_Down) {
            var next = selectedIndex + cols
            if (next < searchResults.length) selectedIndex = next
            event.accepted = true
        } else if (event.key === Qt.Key_K || event.key === Qt.Key_Up) {
            var prev = selectedIndex - cols
            if (prev >= 0) selectedIndex = prev
            event.accepted = true
        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Space) {
            if (selectedIndex >= 0) {
                previewDrawer.openPreview(selectedIndex)
                keyboardMode = "PREVIEW"
            }
            event.accepted = true
        } else if (event.key === Qt.Key_F) {
            if (selectedIndex >= 0) favoriteItem(selectedIndex)
            event.accepted = true
        } else if (event.key === Qt.Key_I || event.key === Qt.Key_Slash) {
            keyboardMode = "SEARCH"
            event.accepted = true
        } else if (event.key === Qt.Key_Q) {
            window.close()
            event.accepted = true
        } else if (event.key === Qt.Key_PageDown) {
            filterBar.nextPage()
            event.accepted = true
        } else if (event.key === Qt.Key_PageUp) {
            filterBar.prevPage()
            event.accepted = true
        } else if (event.key >= Qt.Key_1 && event.key <= Qt.Key_4) {
            var idx = event.key - Qt.Key_1
            if (idx < sourceList.length) {
                filterBar.selectSource(sourceList[idx].name)
            }
            event.accepted = true
        }
    }

    function handlePreviewKeys(event) {
        if (event.key === Qt.Key_Escape || event.key === Qt.Key_Q) {
            previewDrawer.close()
            keyboardMode = "GRID"
            event.accepted = true
        } else if (event.key === Qt.Key_H || event.key === Qt.Key_Left) {
            if (selectedIndex > 0) {
                selectedIndex--
                previewDrawer.openPreview(selectedIndex)
            }
            event.accepted = true
        } else if (event.key === Qt.Key_L || event.key === Qt.Key_Right) {
            if (selectedIndex < searchResults.length - 1) {
                selectedIndex++
                previewDrawer.openPreview(selectedIndex)
            }
            event.accepted = true
        } else if (event.key === Qt.Key_F) {
            if (selectedIndex >= 0) favoriteItem(selectedIndex)
            event.accepted = true
        } else if (event.key === Qt.Key_O) {
            if (selectedIndex >= 0 && searchResults[selectedIndex]) {
                Qt.openUrlExternally(searchResults[selectedIndex].source_url)
            }
            event.accepted = true
        }
    }

    function executeSearch(query, source, page, aspect) {
        loading = true
        selectedIndex = -1
        var args = ["search"]
        if (query && query.length > 0) args.push(query)
        if (source && source !== "All") {
            args.push("--source")
            args.push(source)
        }
        args.push("--page")
        args.push(page.toString())
        args.push("--per-page")
        args.push("24")
        if (aspect && aspect !== "all") {
            args.push("--aspect")
            args.push(aspect)
        }
        CLI.run("search", args)
    }

    function favoriteItem(idx) {
        if (idx < 0 || idx >= searchResults.length) return
        var item = searchResults[idx]
        if (item.is_favorited) return
        CLI.run("fav-" + idx, ["favorites", "add", item.source_url])
    }

    // Layout
    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        FilterBar {
            id: filterBar
            Layout.fillWidth: true
        }

        SearchView {
            id: searchView
            Layout.fillWidth: true
            Layout.fillHeight: true
        }

        StatusBar {
            id: statusBar
            Layout.fillWidth: true
        }
    }

    PreviewDrawer {
        id: previewDrawer
    }
}
