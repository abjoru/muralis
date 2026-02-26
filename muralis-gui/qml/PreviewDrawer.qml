import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import QtQuick.Window

Drawer {
    id: root
    edge: Qt.RightEdge
    width: parent.width * 0.35
    height: parent.height
    modal: false
    interactive: false

    background: Rectangle {
        color: Theme.surfaceContainer
        Rectangle {
            anchors.left: parent.left
            width: 1
            height: parent.height
            color: Theme.withAlpha(Theme.outline, 0.2)
        }
    }

    property var currentItem: null
    property bool showMonitorOverlay: true

    function openPreview(idx) {
        if (idx >= 0 && idx < window.searchResults.length) {
            currentItem = window.searchResults[idx]
            open()
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Theme.spacingL
        spacing: Theme.spacingL

        // Header
        RowLayout {
            Layout.fillWidth: true
            Label {
                text: "Preview"
                font.pixelSize: 18
                font.bold: true
                color: Theme.surfaceText
                Layout.fillWidth: true
            }
            ToolButton {
                text: "\u2715"
                font.pixelSize: 16
                onClicked: {
                    root.close()
                    window.keyboardMode = "GRID"
                }
                Material.foreground: Theme.surfaceText
            }
        }

        // Preview image with monitor overlay
        Rectangle {
            id: previewContainer
            Layout.fillWidth: true
            Layout.preferredHeight: width * 0.6
            radius: Theme.cornerRadius
            color: Theme.surfaceContainerHigh
            clip: true

            Image {
                id: previewImage
                anchors.fill: parent
                source: root.currentItem ? root.currentItem.full_url : ""
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                cache: true

                BusyIndicator {
                    anchors.centerIn: parent
                    running: previewImage.status === Image.Loading
                    Material.accent: Theme.primary
                }
            }

            // Monitor crop overlay
            Item {
                id: monitorOverlay
                anchors.fill: parent
                visible: root.showMonitorOverlay && previewImage.status === Image.Ready && root.currentItem

                // Compute the painted image rect within the container
                property real imgW: root.currentItem ? root.currentItem.width : 1
                property real imgH: root.currentItem ? root.currentItem.height : 1
                property real imgAspect: imgW / imgH
                property real containerAspect: previewContainer.width / Math.max(1, previewContainer.height)

                // Painted image dimensions (PreserveAspectFit)
                property real paintedW: imgAspect > containerAspect ? previewContainer.width : previewContainer.height * imgAspect
                property real paintedH: imgAspect > containerAspect ? previewContainer.width / imgAspect : previewContainer.height
                property real paintedX: (previewContainer.width - paintedW) / 2
                property real paintedY: (previewContainer.height - paintedH) / 2

                // Monitor dimensions
                property real monW: Screen.width
                property real monH: Screen.height
                property real monAspect: monW / monH

                // Crop region in image-space (fill mode crops to fit monitor)
                // visibleW/visibleH = fraction of image that would be visible
                property real visibleFracW: Math.min(1.0, monAspect / imgAspect)
                property real visibleFracH: Math.min(1.0, imgAspect / monAspect)

                // Map to painted coordinates
                property real cropW: paintedW * visibleFracW
                property real cropH: paintedH * visibleFracH
                property real cropX: paintedX + (paintedW - cropW) / 2
                property real cropY: paintedY + (paintedH - cropH) / 2

                // Dim areas outside the crop (4 rectangles for the letterbox)
                // Top
                Rectangle {
                    x: monitorOverlay.paintedX; y: monitorOverlay.paintedY
                    width: monitorOverlay.paintedW
                    height: monitorOverlay.cropY - monitorOverlay.paintedY
                    color: Theme.withAlpha("#000000", 0.55)
                }
                // Bottom
                Rectangle {
                    x: monitorOverlay.paintedX
                    y: monitorOverlay.cropY + monitorOverlay.cropH
                    width: monitorOverlay.paintedW
                    height: (monitorOverlay.paintedY + monitorOverlay.paintedH) - (monitorOverlay.cropY + monitorOverlay.cropH)
                    color: Theme.withAlpha("#000000", 0.55)
                }
                // Left
                Rectangle {
                    x: monitorOverlay.paintedX; y: monitorOverlay.cropY
                    width: monitorOverlay.cropX - monitorOverlay.paintedX
                    height: monitorOverlay.cropH
                    color: Theme.withAlpha("#000000", 0.55)
                }
                // Right
                Rectangle {
                    x: monitorOverlay.cropX + monitorOverlay.cropW
                    y: monitorOverlay.cropY
                    width: (monitorOverlay.paintedX + monitorOverlay.paintedW) - (monitorOverlay.cropX + monitorOverlay.cropW)
                    height: monitorOverlay.cropH
                    color: Theme.withAlpha("#000000", 0.55)
                }

                // Crop border
                Rectangle {
                    x: monitorOverlay.cropX; y: monitorOverlay.cropY
                    width: monitorOverlay.cropW; height: monitorOverlay.cropH
                    color: "transparent"
                    border.width: 1
                    border.color: Theme.withAlpha("#d8a657", 0.7)
                }

                // Monitor label
                Rectangle {
                    x: monitorOverlay.cropX + monitorOverlay.cropW - monLabel.width - 8
                    y: monitorOverlay.cropY + 4
                    width: monLabel.width + 8
                    height: monLabel.height + 4
                    radius: 3
                    color: Theme.withAlpha("#000000", 0.6)

                    Label {
                        id: monLabel
                        anchors.centerIn: parent
                        text: Math.round(monitorOverlay.monW) + "x" + Math.round(monitorOverlay.monH)
                        font.pixelSize: 10
                        color: "#d8a657"
                    }
                }
            }
        }

        // Monitor overlay toggle
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacingS

            Switch {
                id: overlaySwitch
                checked: root.showMonitorOverlay
                onCheckedChanged: root.showMonitorOverlay = checked
                Material.accent: Theme.primary
            }
            Label {
                text: "Show monitor crop"
                font.pixelSize: 12
                color: Theme.withAlpha(Theme.surfaceText, 0.7)
                Layout.fillWidth: true
            }
        }

        // Metadata
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: Theme.spacingM
            rowSpacing: Theme.spacingS

            Label {
                text: "Source"
                color: Theme.withAlpha(Theme.surfaceText, 0.6)
                font.pixelSize: 12
            }
            Label {
                text: root.currentItem ? root.currentItem.source_type : ""
                color: Theme.surfaceText
                font.pixelSize: 12
            }

            Label {
                text: "Resolution"
                color: Theme.withAlpha(Theme.surfaceText, 0.6)
                font.pixelSize: 12
            }
            Label {
                text: root.currentItem ? root.currentItem.width + " x " + root.currentItem.height : ""
                color: Theme.surfaceText
                font.pixelSize: 12
            }

            Label {
                text: "Tags"
                color: Theme.withAlpha(Theme.surfaceText, 0.6)
                font.pixelSize: 12
                Layout.alignment: Qt.AlignTop
            }
            Label {
                text: root.currentItem && root.currentItem.tags ? root.currentItem.tags.join(", ") : ""
                color: Theme.surfaceText
                font.pixelSize: 12
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }

        // Action buttons
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacingM

            Button {
                id: favBtn
                Layout.fillWidth: true
                text: (root.currentItem && root.currentItem.is_favorited) ? "\u2605 Favorited" : "\u2606 Add to Favorites"
                enabled: !(root.currentItem && root.currentItem.is_favorited)
                Material.accent: Theme.primary
                Material.background: Theme.primaryContainer
                Material.foreground: Theme.surfaceText

                onClicked: {
                    if (window.selectedIndex >= 0) {
                        window.favoriteItem(window.selectedIndex)
                    }
                }
            }

            Button {
                Layout.fillWidth: true
                text: "Open URL"
                flat: true
                Material.foreground: Theme.primary
                onClicked: {
                    if (root.currentItem) {
                        Qt.openUrlExternally(root.currentItem.source_url)
                    }
                }
            }
        }

        // Spacer
        Item { Layout.fillHeight: true }
    }
}
