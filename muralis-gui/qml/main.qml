import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import MuralisGui
import "Retrieval.js" as Retrieval

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

    // Keyboard mode state machine: SEARCH / BROWSE / GRID / PREVIEW
    property string keyboardMode: "SEARCH"
    property var searchResults: []
    property var sourceList: []
    property int selectedIndex: -1
    property bool loading: false
    // A retrieval has been issued and answered — distinguishes "nothing yet"
    // from "nothing found".
    property bool hasRetrieved: false
    property string retrievalError: ""

    // Load sources on startup
    Component.onCompleted: {
        CLI.run("sources", ["sources", "list"])
        if (InitialQuery && InitialQuery.length > 0) {
            filterBar.searchText = InitialQuery
            // Delay search to ensure UI is ready
            initialSearchTimer.start()
        } else {
            filterBar.focusSearch()
        }
    }

    Timer {
        id: initialSearchTimer
        interval: 500
        onTriggered: filterBar.executeSearch()
    }

    onKeyboardModeChanged: {
        if (keyboardMode === "SEARCH") {
            // A browsed source offers no query field to focus; browsing the
            // category bar is the equivalent mode.
            if (filterBar.queryVisible) filterBar.focusSearch()
            else keyboardMode = categoryBar.visible ? "BROWSE" : "GRID"
        } else if (keyboardMode === "BROWSE") {
            if (categoryBar.visible) categoryBar.focusBar()
            else keyboardMode = "GRID"
        } else {
            gridFocus.forceActiveFocus()
        }
    }

    Connections {
        target: CLI
        function onFinished(requestId, stdout, stderr, exitCode) {
            if (exitCode !== 0) {
                console.error("CLI failed:", requestId, stderr || stdout)
                if (requestId === "retrieve") {
                    // A failed retrieval says so; it must not pass for an
                    // empty category.
                    retrievalError = firstLine(stderr) || "Retrieval failed"
                    searchResults = []
                }
                loading = false
                return
            }

            if (requestId === "sources") {
                try {
                    sourceList = JSON.parse(stdout)
                } catch (e) {
                    console.error("Failed to parse sources:", e)
                }
            } else if (requestId === "retrieve") {
                try {
                    var data = JSON.parse(stdout)
                    searchResults = data.results || []
                    searchView.hasMore = data.has_more || false
                } catch (e) {
                    console.error("Failed to parse results:", e)
                    searchResults = []
                    retrievalError = "Could not read the retrieval's output"
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

    function firstLine(text) {
        if (!text) return ""
        return text.split("\n")[0].trim()
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
        } else if (event.key === Qt.Key_C) {
            if (categoryBar.visible) keyboardMode = "BROWSE"
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
        } else if (event.key >= Qt.Key_1 && event.key <= Qt.Key_9) {
            // Every configured source is reachable by number, browsed ones
            // included — selecting one opens its category bar.
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

    function clearResults() {
        searchResults = []
        searchView.hasMore = false
        selectedIndex = -1
        hasRetrieved = false
        retrievalError = ""
    }

    // Issue one retrieval. Which verb answers — search or browse — follows from
    // the selected source's declared retrieval mode.
    function retrieve(req) {
        var args = Retrieval.args(sourceList, req)
        if (!args) return

        loading = true
        hasRetrieved = true
        retrievalError = ""
        selectedIndex = -1
        CLI.run("retrieve", args)
    }

    function favoriteItem(idx) {
        if (idx < 0 || idx >= searchResults.length) return
        var argv = Retrieval.keepArgs(searchResults[idx])
        if (!argv) return
        CLI.run("fav-" + idx, argv)
    }

    // Layout
    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        FilterBar {
            id: filterBar
            Layout.fillWidth: true
        }

        CategoryBar {
            id: categoryBar
            Layout.fillWidth: true
            visible: filterBar.isBrowsedSource && filterBar.activeCategories.length > 0
            categories: filterBar.activeCategories
            activeCategory: filterBar.activeCategory
            onSelected: function(slug) { filterBar.selectCategory(slug) }
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
