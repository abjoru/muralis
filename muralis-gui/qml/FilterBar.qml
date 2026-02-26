import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

Rectangle {
    id: root
    height: 48
    color: Theme.surfaceContainer
    clip: true

    property alias searchText: searchField.text
    property string activeSource: "All"
    property int currentPage: 1
    property string activeAspect: "all"

    function focusSearch() {
        searchField.forceActiveFocus()
    }

    function clearSearch() {
        searchField.text = ""
    }

    function executeSearch() {
        if (searchField.text.length > 0 || activeAspect !== "all") {
            currentPage = 1
            window.executeSearch(searchField.text, activeSource, currentPage, activeAspect)
        }
    }

    function nextPage() {
        if (searchView.hasMore) {
            currentPage++
            window.executeSearch(searchField.text, activeSource, currentPage, activeAspect)
        }
    }

    function prevPage() {
        if (currentPage > 1) {
            currentPage--
            window.executeSearch(searchField.text, activeSource, currentPage, activeAspect)
        }
    }

    property var apiSources: {
        var result = []
        for (var i = 0; i < window.sourceList.length; i++)
            if (window.sourceList[i].source_type !== "feed")
                result.push(window.sourceList[i])
        return result
    }

    property var feedSources: {
        var result = []
        for (var i = 0; i < window.sourceList.length; i++)
            if (window.sourceList[i].source_type === "feed")
                result.push(window.sourceList[i])
        return result
    }

    property bool isFeedSource: {
        if (activeSource === "All") return false
        for (var i = 0; i < feedSources.length; i++)
            if (feedSources[i].name === activeSource) return true
        return false
    }

    function selectSource(name) {
        activeSource = name
        // Reset feed combo when selecting non-feed source
        if (!isFeedSource) feedCombo.currentIndex = 0
        // Feeds load immediately, others need query or aspect
        if (isFeedSource) {
            currentPage = 1
            window.executeSearch("", activeSource, 1, "all")
        } else if (searchField.text.length > 0 || activeAspect !== "all") {
            executeSearch()
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.spacingL
        anchors.rightMargin: Theme.spacingL
        spacing: Theme.spacingS

        // Source chip buttons (API sources only)
        Repeater {
            model: {
                var items = [{ name: "All" }]
                for (var i = 0; i < root.apiSources.length; i++)
                    items.push(root.apiSources[i])
                return items
            }

            Rectangle {
                Layout.preferredWidth: chipLabel.implicitWidth + Theme.spacingM * 2
                Layout.preferredHeight: 22
                Layout.alignment: Qt.AlignVCenter
                radius: 4
                color: activeSource === modelData.name ? Theme.primaryContainer : "transparent"

                Label {
                    id: chipLabel
                    anchors.centerIn: parent
                    text: modelData.name
                    font.pixelSize: 11
                    font.bold: activeSource === modelData.name
                    color: activeSource === modelData.name ? Theme.surfaceText : Theme.withAlpha(Theme.surfaceText, 0.6)
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: selectSource(modelData.name)
                }
            }
        }

        // Feed source dropdown
        ComboBox {
            id: feedCombo
            visible: root.feedSources.length > 0
            Layout.preferredHeight: 32
            Layout.alignment: Qt.AlignVCenter
            font.pixelSize: 13
            model: {
                var items = ["Feeds..."]
                for (var i = 0; i < root.feedSources.length; i++)
                    items.push(root.feedSources[i].name)
                return items
            }
            Material.accent: Theme.primary
            Material.foreground: Theme.surfaceText
            onActivated: function(index) {
                if (index > 0) selectSource(currentText)
            }
        }

        // Spacer
        Item { Layout.fillWidth: true }

        // Search field
        TextField {
            id: searchField
            visible: !root.isFeedSource
            Layout.preferredWidth: 300
            Layout.preferredHeight: 32
            Layout.alignment: Qt.AlignVCenter
            verticalAlignment: TextInput.AlignVCenter
            topPadding: 0
            bottomPadding: 0
            color: Theme.surfaceText
            font.pixelSize: 13

            Material.accent: Theme.primary
            Material.containerStyle: Material.Filled

            Label {
                anchors.verticalCenter: parent.verticalCenter
                anchors.left: parent.left
                anchors.leftMargin: searchField.leftPadding
                text: "Search wallpapers..."
                color: Theme.withAlpha(Theme.surfaceText, 0.5)
                font.pixelSize: 13
                visible: searchField.text.length === 0 && !searchField.activeFocus
            }

            onActiveFocusChanged: {
                if (activeFocus) window.keyboardMode = "SEARCH"
            }

            Timer {
                id: debounce
                interval: 300
                onTriggered: {
                    if (searchField.text.length > 0) {
                        root.currentPage = 1
                        window.executeSearch(searchField.text, root.activeSource, 1, root.activeAspect)
                    }
                }
            }

            onTextChanged: debounce.restart()

            Keys.onReturnPressed: root.executeSearch()
            Keys.onEnterPressed: root.executeSearch()
            Keys.onEscapePressed: {
                if (text.length > 0) {
                    text = ""
                } else {
                    window.keyboardMode = "GRID"
                    focus = false
                }
            }
            Keys.onTabPressed: {
                window.keyboardMode = "GRID"
                if (window.selectedIndex < 0 && window.searchResults.length > 0)
                    window.selectedIndex = 0
                focus = false
            }
        }

        // Aspect ratio filter
        ComboBox {
            id: aspectCombo
            visible: !root.isFeedSource
            Layout.preferredHeight: 32
            Layout.alignment: Qt.AlignVCenter
            model: ["All", "16:9", "21:9", "32:9", "16:10", "4:3", "3:2"]
            font.pixelSize: 13
            Material.accent: Theme.primary
            Material.foreground: Theme.surfaceText
            onCurrentTextChanged: {
                var map = {
                    "All": "all", "16:9": "16x9", "21:9": "21x9",
                    "32:9": "32x9", "16:10": "16x10", "4:3": "4x3", "3:2": "3x2"
                }
                root.activeAspect = map[currentText] || "all"
                if (searchField.text.length > 0 || root.activeAspect !== "all") {
                    root.currentPage = 1
                    window.executeSearch(searchField.text, root.activeSource, 1, root.activeAspect)
                }
            }
        }
    }

    // Bottom border
    Rectangle {
        anchors.bottom: parent.bottom
        width: parent.width
        height: 1
        color: Theme.withAlpha(Theme.outline, 0.2)
    }
}
