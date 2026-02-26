import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

Rectangle {
    id: root
    height: 32
    color: Theme.surfaceContainer

    // Top border
    Rectangle {
        anchors.top: parent.top
        width: parent.width
        height: 1
        color: Theme.withAlpha(Theme.outline, 0.2)
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.spacingL
        anchors.rightMargin: Theme.spacingL
        spacing: Theme.spacingM

        // Mode badge
        Rectangle {
            Layout.preferredWidth: modeLabel.implicitWidth + Theme.spacingM * 2
            Layout.preferredHeight: 22
            radius: 4
            color: Theme.primaryContainer

            Label {
                id: modeLabel
                anchors.centerIn: parent
                text: "[" + window.keyboardMode + "]"
                font.pixelSize: 11
                font.bold: true
                font.family: Theme.monoFontFamily
                color: Theme.surfaceText
            }
        }

        // Spacer
        Item { Layout.fillWidth: true }

        // Pagination
        Row {
            spacing: Theme.spacingXS
            visible: window.searchResults.length > 0

            Button {
                text: "<"
                flat: true
                enabled: filterBar.currentPage > 1
                font.pixelSize: 11
                implicitWidth: 28
                implicitHeight: 22
                topPadding: 0; bottomPadding: 0; leftPadding: 4; rightPadding: 4
                onClicked: filterBar.prevPage()
            }

            Label {
                anchors.verticalCenter: parent.verticalCenter
                text: "Page " + filterBar.currentPage
                font.pixelSize: 11
                color: Theme.withAlpha(Theme.surfaceText, 0.7)
            }

            Button {
                text: ">"
                flat: true
                enabled: searchView.hasMore
                font.pixelSize: 11
                implicitWidth: 28
                implicitHeight: 22
                topPadding: 0; bottomPadding: 0; leftPadding: 4; rightPadding: 4
                onClicked: filterBar.nextPage()
            }

            Label {
                anchors.verticalCenter: parent.verticalCenter
                text: "·  " + window.searchResults.length + " results"
                font.pixelSize: 11
                color: Theme.withAlpha(Theme.surfaceText, 0.7)
            }
        }

        // Spacer
        Item { Layout.fillWidth: true }

        // Key hints
        Label {
            font.pixelSize: 11
            font.family: Theme.monoFontFamily
            color: Theme.withAlpha(Theme.surfaceText, 0.5)
            text: {
                switch (window.keyboardMode) {
                case "SEARCH":
                    return "Enter:search  Tab:grid  Esc:clear"
                case "GRID":
                    return "hjkl:nav  Enter:preview  f:fav  PgDn/PgUp:page  /:search  q:quit"
                case "PREVIEW":
                    return "h/l:prev/next  f:fav  o:open  Esc:close"
                default:
                    return ""
                }
            }
        }
    }
}
