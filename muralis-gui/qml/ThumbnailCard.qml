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
            source: root.thumbnailUrl
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            cache: true

            // Loading placeholder
            Rectangle {
                anchors.fill: parent
                color: Theme.surfaceContainer
                visible: thumb.status !== Image.Ready

                BusyIndicator {
                    anchors.centerIn: parent
                    running: thumb.status === Image.Loading
                    width: 24
                    height: 24
                    Material.accent: Theme.primary
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
