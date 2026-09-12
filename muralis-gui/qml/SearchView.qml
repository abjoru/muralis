import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import "Retrieval.js" as Retrieval

Item {
    id: root

    property real gridWidth: width - 2 * Theme.spacingS
    property int columns: Math.max(3, Math.min(8, Math.floor(gridWidth / 200)))
    property real cellSize: gridWidth / columns
    property bool hasMore: false

    readonly property var message: Retrieval.gridMessage(window.sourceList, {
        source: filterBar.activeSource,
        category: filterBar.activeCategory,
        loading: window.loading,
        retrieved: window.hasRetrieved,
        resultCount: window.searchResults.length,
        error: window.retrievalError
    })

    GridView {
        id: grid
        anchors.fill: parent
        anchors.margins: Theme.spacingS
        cellWidth: root.cellSize
        cellHeight: root.cellSize * 0.65
        clip: true

        model: window.searchResults

        delegate: ThumbnailCard {
            width: grid.cellWidth - Theme.spacingS
            height: grid.cellHeight - Theme.spacingS
            thumbnailUrl: modelData.thumbnail_url
            resolution: modelData.width + "x" + modelData.height
            isFavorited: modelData.is_favorited || false
            isSelected: index === window.selectedIndex

            onClicked: {
                window.selectedIndex = index
                window.keyboardMode = "GRID"
            }

            onDoubleClicked: {
                window.selectedIndex = index
                previewDrawer.openPreview(index)
                window.keyboardMode = "PREVIEW"
            }
        }

        ScrollBar.vertical: ScrollBar {
            policy: ScrollBar.AsNeeded
        }

        // Empty, awaiting-a-category and failed states — each distinct from
        // the loading indicator below.
        Label {
            anchors.centerIn: parent
            width: parent.width - Theme.spacingXL * 2
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            visible: text.length > 0 && window.searchResults.length === 0
            text: root.message.text
            color: root.message.isError ? Theme.error : Theme.withAlpha(Theme.surfaceText, 0.5)
            font.pixelSize: 16
        }

        // Loading indicator
        BusyIndicator {
            anchors.centerIn: parent
            running: window.loading
            visible: window.loading
            Material.accent: Theme.primary
        }
    }

    // Ensure selected item is visible
    onVisibleChanged: {
        if (window.selectedIndex >= 0)
            grid.positionViewAtIndex(window.selectedIndex, GridView.Contain)
    }

    Connections {
        target: window
        function onSelectedIndexChanged() {
            if (window.selectedIndex >= 0)
                grid.positionViewAtIndex(window.selectedIndex, GridView.Contain)
        }
    }
}
