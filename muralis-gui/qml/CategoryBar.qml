import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import "Retrieval.js" as Retrieval

// The categories a browsed source publishes, by display label. Where the
// source declares its categories combine, the chips toggle and the selection
// is a set — the intersection is the slice. Where it declares they do not, the
// bar is what it always was: one chip at a time, replaced on each pick.
//
// The bar holds no selection of its own. Picking reports upwards and the
// selection comes back down, so what the chips show and what was asked for
// cannot drift apart.
Rectangle {
    id: root
    height: 36
    color: Theme.surfaceVariant
    clip: true

    property var categories: []
    property var selection: []
    property bool combines: false

    // Keyboard cursor, independent of what is currently loaded: moving it does
    // not retrieve, and neither does toggling — pressing Enter does.
    property int highlightIndex: 0

    // `settle` asks for the selection to be retrieved once it stops moving;
    // without it the selection changes and nothing is asked for yet.
    signal picked(string slug, bool settle)
    signal cleared()
    // Retrieve the selection built so far, now.
    signal committed()

    function focusBar() {
        highlightIndex = Math.max(0, indexOfSlug(selection.length > 0 ? selection[0] : ""))
        forceActiveFocus()
    }

    function indexOfSlug(slug) {
        for (var i = 0; i < categories.length; i++)
            if (categories[i].slug === slug) return i
        return -1
    }

    function isSelected(slug) {
        return selection.indexOf(slug) >= 0
    }

    function highlightedSlug() {
        if (highlightIndex < 0 || highlightIndex >= categories.length) return ""
        return categories[highlightIndex].slug
    }

    // The vocabulary is wider than the bar, so the cursor is scrolled to rather
    // than left off the end of it — otherwise moving the highlight past the
    // edge is a keystroke with nothing to show for it.
    function ensureHighlightVisible() {
        var chip = chipRepeater.itemAt(highlightIndex)
        if (!chip) return
        var left = chipRow.x + chip.x
        if (left < flick.contentX)
            flick.contentX = Math.max(0, left)
        else if (left + chip.width > flick.contentX + flick.width)
            flick.contentX = Math.min(flick.contentWidth - flick.width,
                                      left + chip.width - flick.width)
    }

    // A mouse pick always asks for a retrieval; several in succession settle
    // into one. A source that does not combine leaves the bar afterwards, the
    // way picking its one category always has.
    function pickWithMouse(slug) {
        root.picked(slug, true)
        if (!combines) window.keyboardMode = "GRID"
    }

    onCategoriesChanged: {
        highlightIndex = 0
        flick.contentX = 0
    }
    onHighlightIndexChanged: ensureHighlightVisible()
    // Selecting widens the pinned summary and so narrows the chips beside it;
    // the cursor has to be brought back into what is left.
    onSelectionChanged: ensureHighlightVisible()

    activeFocusOnTab: true

    Keys.onPressed: function (event) {
        if (event.key === Qt.Key_H || event.key === Qt.Key_Left) {
            if (highlightIndex > 0) highlightIndex--
            event.accepted = true
        } else if (event.key === Qt.Key_L || event.key === Qt.Key_Right) {
            if (highlightIndex < categories.length - 1) highlightIndex++
            event.accepted = true
        } else if (event.key === Qt.Key_Space) {
            // Toggle, distinct from commit: a multi-chip selection can be built
            // without a retrieval per keystroke. Where categories do not
            // combine there is nothing to build, so the pick stands alone.
            var slug = highlightedSlug()
            if (slug !== "") {
                root.picked(slug, !combines)
                if (!combines) window.keyboardMode = "GRID"
            }
            event.accepted = true
        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            // Commit what is selected. A highlighted chip nobody toggled joins
            // the selection first, so Enter alone still loads a category.
            var here = highlightedSlug()
            if (here !== "" && !isSelected(here)) root.picked(here, false)
            root.committed()
            window.keyboardMode = "GRID"
            event.accepted = true
        } else if (event.key === Qt.Key_X || event.key === Qt.Key_Delete
                   || event.key === Qt.Key_Backspace) {
            if (selection.length > 0) root.cleared()
            event.accepted = true
        } else if (event.key === Qt.Key_Escape || event.key === Qt.Key_Tab) {
            window.keyboardMode = "GRID"
            event.accepted = true
        } else if (event.key === Qt.Key_Q) {
            window.close()
            event.accepted = true
        }
    }

    // How many categories are active, which ones, and the way to drop them —
    // all three pinned outside the Flickable. The vocabulary is wider than the
    // bar, so anything living among the chips can be scrolled off the end of
    // it. The slot's width is fixed rather than grown from the selection:
    // selecting must not shift every chip out from under the pointer mid-burst.
    Item {
        id: pinned
        anchors.left: parent.left
        anchors.leftMargin: Theme.spacingL
        anchors.verticalCenter: parent.verticalCenter
        width: Math.max(150, Math.min(380, root.width * 0.26))
        height: 22

        Label {
            id: summary
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            width: pinned.width - clearChip.width - Theme.spacingS
            elide: Text.ElideRight
            text: root.selection.length > 0
                  ? "Categories (" + root.selection.length + "): "
                    + Retrieval.selectionLabel(window.sourceList, filterBar.activeSource, root.selection)
                  : "Categories:"
            font.pixelSize: 11
            font.bold: root.selection.length > 0
            color: root.selection.length > 0
                   ? Theme.surfaceText : Theme.withAlpha(Theme.surfaceText, 0.5)
        }

        // Clearing the whole selection, in one action. Its space is reserved
        // whether or not it is showing, for the same reason.
        Rectangle {
            id: clearChip
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            visible: root.selection.length > 0
            width: 54
            height: 22
            radius: 4
            color: "transparent"
            border.width: 1
            border.color: Theme.withAlpha(Theme.outline, 0.3)

            Label {
                anchors.centerIn: parent
                text: "✕ Clear"
                font.pixelSize: 11
                color: Theme.withAlpha(Theme.surfaceText, 0.6)
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.cleared()
            }
        }
    }

    Flickable {
        id: flick
        anchors.left: pinned.right
        anchors.leftMargin: Theme.spacingM
        anchors.right: parent.right
        anchors.rightMargin: Theme.spacingL
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        contentWidth: chipRow.width
        flickableDirection: Flickable.HorizontalFlick
        clip: true
        onWidthChanged: root.ensureHighlightVisible()

        Row {
            id: chipRow
            height: parent.height
            spacing: Theme.spacingS

            Repeater {
                id: chipRepeater
                model: root.categories

                Rectangle {
                    id: chip
                    readonly property bool selected: root.isSelected(modelData.slug)
                    readonly property bool highlighted: root.activeFocus && root.highlightIndex === index

                    anchors.verticalCenter: parent.verticalCenter
                    width: categoryLabel.implicitWidth + Theme.spacingM * 2
                    height: 22
                    radius: 4
                    // Selected is a filled chip, not a tint: the difference
                    // reads across the bar at a glance.
                    color: selected ? Theme.primary
                                    : (highlighted ? Theme.surfaceContainerHighest : "transparent")
                    border.width: selected ? 0 : 1
                    border.color: highlighted ? Theme.primary : Theme.withAlpha(Theme.outline, 0.3)

                    Label {
                        id: categoryLabel
                        anchors.centerIn: parent
                        text: (chip.selected ? "✓ " : "") + (modelData.label || modelData.slug)
                        font.pixelSize: 11
                        font.bold: chip.selected
                        color: chip.selected ? Theme.primaryText
                                             : Theme.withAlpha(Theme.surfaceText, 0.7)
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.highlightIndex = index
                            root.pickWithMouse(modelData.slug)
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
