import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material

// The categories a browsed source publishes, by display label. Selecting one
// retrieves its first page — for a categorised source, picking the category is
// the selection, the way picking the feed is for a feed.
Rectangle {
    id: root
    height: 36
    color: Theme.surfaceVariant
    clip: true

    property var categories: []
    property string activeCategory: ""

    // Keyboard cursor, independent of what is currently loaded: moving it does
    // not retrieve, pressing Enter does.
    property int highlightIndex: 0

    signal selected(string slug)

    function focusBar() {
        highlightIndex = Math.max(0, indexOfSlug(activeCategory))
        forceActiveFocus()
    }

    function indexOfSlug(slug) {
        for (var i = 0; i < categories.length; i++)
            if (categories[i].slug === slug) return i
        return -1
    }

    function selectHighlighted() {
        if (highlightIndex >= 0 && highlightIndex < categories.length)
            root.selected(categories[highlightIndex].slug)
    }

    onCategoriesChanged: highlightIndex = Math.max(0, indexOfSlug(activeCategory))

    activeFocusOnTab: true

    Keys.onPressed: function (event) {
        if (event.key === Qt.Key_H || event.key === Qt.Key_Left) {
            if (highlightIndex > 0) highlightIndex--
            event.accepted = true
        } else if (event.key === Qt.Key_L || event.key === Qt.Key_Right) {
            if (highlightIndex < categories.length - 1) highlightIndex++
            event.accepted = true
        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter
                   || event.key === Qt.Key_Space) {
            selectHighlighted()
            event.accepted = true
        } else if (event.key === Qt.Key_Escape || event.key === Qt.Key_Tab) {
            window.keyboardMode = "GRID"
            event.accepted = true
        } else if (event.key === Qt.Key_Q) {
            window.close()
            event.accepted = true
        }
    }

    Flickable {
        anchors.fill: parent
        anchors.leftMargin: Theme.spacingL
        anchors.rightMargin: Theme.spacingL
        contentWidth: chipRow.width
        flickableDirection: Flickable.HorizontalFlick
        clip: true

        Row {
            id: chipRow
            height: parent.height
            spacing: Theme.spacingS

            Label {
                anchors.verticalCenter: parent.verticalCenter
                text: "Categories:"
                font.pixelSize: 11
                color: Theme.withAlpha(Theme.surfaceText, 0.5)
                rightPadding: Theme.spacingXS
            }

            Repeater {
                model: root.categories

                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: categoryLabel.implicitWidth + Theme.spacingM * 2
                    height: 22
                    radius: 4
                    color: root.activeCategory === modelData.slug
                           ? Theme.primaryContainer
                           : (root.activeFocus && root.highlightIndex === index
                              ? Theme.surfaceContainerHighest : "transparent")
                    border.width: root.activeFocus && root.highlightIndex === index ? 1 : 0
                    border.color: Theme.primary

                    Label {
                        id: categoryLabel
                        anchors.centerIn: parent
                        text: modelData.label || modelData.slug
                        font.pixelSize: 11
                        font.bold: root.activeCategory === modelData.slug
                        color: root.activeCategory === modelData.slug
                               ? Theme.surfaceText : Theme.withAlpha(Theme.surfaceText, 0.7)
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.highlightIndex = index
                            root.selected(modelData.slug)
                        }
                    }
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
