import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material

Item {
    id: root

    property real gridWidth: width - 2 * Theme.spacingS
    property int columns: Math.max(3, Math.min(8, Math.floor(gridWidth / 200)))
    property real cellSize: gridWidth / columns
    property bool hasMore: false

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

        // Empty state
        Label {
            anchors.centerIn: parent
            visible: !window.loading && window.searchResults.length === 0
            text: "Search for wallpapers to get started"
            color: Theme.withAlpha(Theme.surfaceText, 0.5)
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
