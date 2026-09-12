import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "Retrieval.js" as Retrieval

Rectangle {
    id: root
    height: 48
    color: Theme.surfaceContainer
    clip: true

    property alias searchText: searchField.text
    property string activeSource: "All"
    property int currentPage: 1
    property string activeAspect: "all"
    // The categories currently selected, as slugs in the source's published
    // order. A set, not one slug: where a source's categories combine, the
    // intersection is the slice.
    property var selectedCategories: []

    // Sources split on their declared retrieval mode, reported by `sources
    // list` — never on a source-type name.
    readonly property var searchedSources: Retrieval.searched(window.sourceList)
    readonly property var browsedSources: Retrieval.browsed(window.sourceList)
    readonly property bool isBrowsedSource: Retrieval.isBrowsedName(window.sourceList, activeSource)
    readonly property var publishedCategories: Retrieval.categoriesOf(window.sourceList, activeSource)
    readonly property bool categoriesCombine: Retrieval.categoriesCombine(window.sourceList, activeSource)

    // A browsed source has no query dimension, so it is offered none.
    readonly property bool queryVisible: !isBrowsedSource

    function focusSearch() {
        searchField.forceActiveFocus()
    }

    function clearSearch() {
        searchField.text = ""
    }

    function retrieve() {
        window.retrieve({
            source: activeSource,
            query: searchField.text,
            categories: selectedCategories,
            page: currentPage,
            perPage: 24,
            aspect: activeAspect
        })
    }

    function executeSearch() {
        if (searchField.text.length > 0 || activeAspect !== "all") {
            currentPage = 1
            retrieve()
        }
    }

    function nextPage() {
        if (searchView.hasMore) {
            currentPage++
            retrieve()
        }
    }

    function prevPage() {
        if (currentPage > 1) {
            currentPage--
            retrieve()
        }
    }

    function selectSource(name) {
        settle.stop()
        activeSource = name
        selectedCategories = []
        currentPage = 1

        if (!isBrowsedSource) {
            browsedCombo.currentIndex = 0
            if (searchField.text.length > 0 || activeAspect !== "all") retrieve()
            return
        }

        window.clearResults()
        if (publishedCategories.length === 0) {
            // A feed publishes no category: selecting it is the selection.
            retrieve()
        } else {
            // A categorised source retrieves nothing until a category is named.
            window.keyboardMode = "BROWSE"
        }
    }

    // Picking one category. The selection moves at once — the chips follow the
    // click — while the retrieval waits for it to stop moving, so a handful of
    // chips picked in succession costs one request for the final selection.
    function pickCategory(slug, settleAfter) {
        selectedCategories = Retrieval.toggleSelection(publishedCategories, selectedCategories,
                                                       slug, categoriesCombine)
        currentPage = 1
        if (settleAfter) settle.restart()
        else settle.stop()
    }

    function clearCategories() {
        selectedCategories = []
        currentPage = 1
        settle.restart()
    }

    // Retrieve the selection as it stands, without waiting for it to settle.
    function commitCategories() {
        settle.stop()
        currentPage = 1
        retrieve()
    }

    Timer {
        id: settle
        interval: Retrieval.SETTLE_MS
        onTriggered: root.retrieve()
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.spacingL
        anchors.rightMargin: Theme.spacingL
        spacing: Theme.spacingS

        // Source chip buttons (searched sources + "All")
        Repeater {
            model: {
                var items = [{ name: "All" }]
                for (var i = 0; i < root.searchedSources.length; i++)
                    items.push(root.searchedSources[i])
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

        // Browsed source selector
        ComboBox {
            id: browsedCombo
            visible: root.browsedSources.length > 0
            Layout.preferredHeight: 32
            Layout.alignment: Qt.AlignVCenter
            font.pixelSize: 13
            model: {
                var items = ["Browse..."]
                for (var i = 0; i < root.browsedSources.length; i++)
                    items.push(root.browsedSources[i].name)
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
            visible: root.queryVisible
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
                    if (searchField.text.length > 0 && root.queryVisible) {
                        root.currentPage = 1
                        root.retrieve()
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
            visible: root.queryVisible
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
                if (root.queryVisible && (searchField.text.length > 0 || root.activeAspect !== "all")) {
                    root.currentPage = 1
                    root.retrieve()
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
