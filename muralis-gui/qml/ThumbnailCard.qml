import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material

Item {
    id: root

    property string thumbnailUrl: ""
    property string resolution: ""
    property bool isFavorited: false
    property bool isSelected: false

    signal clicked()
    signal doubleClicked()

    // A card renders the local copy, never the remote URL: the fetch that fills
    // the cache is identified as muralis and happens once per thumbnail, where
    // binding an Image straight to the URL is anonymous and fetched every time.
    property string localSource: ""
    property bool fetchFailed: false

    function resolveThumbnail() {
        fetchFailed = false
        localSource = thumbnailUrl ? Thumbnails.request(thumbnailUrl) : ""
    }

    onThumbnailUrlChanged: resolveThumbnail()
    Component.onCompleted: resolveThumbnail()

    Connections {
        target: Thumbnails
        function onReady(remoteUrl, localUrl) {
            if (remoteUrl === root.thumbnailUrl) root.localSource = localUrl
        }
        function onFailed(remoteUrl, reason) {
            if (remoteUrl === root.thumbnailUrl) root.fetchFailed = true
        }
    }

    Rectangle {
        id: card
        anchors.fill: parent
        anchors.margins: Theme.spacingXS / 2
        radius: Theme.cornerRadius
        color: Theme.surfaceContainerHigh
        clip: true

        border.width: root.isSelected ? 2 : 0
        border.color: "#d8a657"

        // Thumbnail image
        Image {
            id: thumb
            anchors.fill: parent
            anchors.margins: root.isSelected ? 2 : 0
            source: root.localSource
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            cache: true

            // Loading and failed are two states, not one blank card: a
            // thumbnail that will never arrive says so instead of spinning.
            readonly property bool broken: root.fetchFailed || thumb.status === Image.Error

            Rectangle {
                anchors.fill: parent
                color: Theme.surfaceContainer
                visible: thumb.status !== Image.Ready

                BusyIndicator {
                    anchors.centerIn: parent
                    running: !thumb.broken && thumb.status !== Image.Ready
                    visible: running
                    width: 24
                    height: 24
                    Material.accent: Theme.primary
                }

                Label {
                    anchors.centerIn: parent
                    visible: thumb.broken
                    text: "\u26A0"
                    font.pixelSize: 22
                    color: Theme.withAlpha(Theme.surfaceText, 0.5)
                }
            }
        }

        // Resolution label
        Rectangle {
            anchors.bottom: parent.bottom
            anchors.right: parent.right
            anchors.margins: Theme.spacingXS
            width: resLabel.implicitWidth + Theme.spacingS
            height: resLabel.implicitHeight + Theme.spacingXS
            radius: 4
            color: Theme.withAlpha(Theme.surface, 0.8)
            visible: root.resolution !== "0x0"

            Label {
                id: resLabel
                anchors.centerIn: parent
                text: root.resolution
                font.pixelSize: 10
                color: Theme.surfaceText
            }
        }

        // Favorite badge
        Rectangle {
            anchors.top: parent.top
            anchors.right: parent.right
            anchors.margins: Theme.spacingXS
            width: 24
            height: 24
            radius: 12
            color: Theme.withAlpha(Theme.surface, 0.8)
            visible: root.isFavorited

            Label {
                anchors.centerIn: parent
                text: "\u2605"
                font.pixelSize: 14
                color: Theme.secondary
            }
        }

        // Hover effect
        Rectangle {
            anchors.fill: parent
            color: mouseArea.containsMouse ? Theme.withAlpha(Theme.surfaceText, 0.08) : "transparent"
            Behavior on color { ColorAnimation { duration: 150 } }
        }

        MouseArea {
            id: mouseArea
            anchors.fill: parent
            hoverEnabled: true
            onClicked: root.clicked()
            onDoubleClicked: root.doubleClicked()
        }
    }
}
