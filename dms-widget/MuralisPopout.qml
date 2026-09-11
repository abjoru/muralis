pragma ComponentBehavior: Bound

import QtQuick
import qs.Common
import qs.Widgets
import qs.Modules.Plugins

// The popout: a page of the Library, transport, and the mode switcher.
//
// A pure view over MuralisWidget's state. It owns no socket and caches
// nothing — everything it draws is a property of `widget`, so a rotation
// arriving over the subscription redraws this without it asking.
PopoutComponent {
    id: popout

    // The MuralisWidget root.
    property var widget: null

    readonly property int columns: 4
    readonly property real cellWidth: (width - (columns - 1) * Theme.spacingS) / columns
    readonly property real cellHeight: Math.round(cellWidth * 9 / 16)

    spacing: Theme.spacingS

    headerText: "Muralis"
    detailsText: {
        if (!widget)
            return ""
        if (!widget.daemonUp)
            return "Daemon not running"
        const count = widget.wallpaperCount + (widget.wallpaperCount === 1 ? " wallpaper" : " wallpapers")
        return widget.modeLabel(widget.mode) + (widget.paused ? " · paused" : "") + " · " + count
    }
    showCloseButton: true

    headerActions: Component {
        DankActionButton {
            iconName: "refresh"
            iconSize: Theme.iconSize - 6
            enabled: popout.widget && popout.widget.daemonUp
            opacity: enabled ? 1 : 0.4
            tooltipText: "Reload the library"
            onClicked: {
                if (popout.widget)
                    popout.widget.refresh()
            }
        }
    }

    // --- Failure states ---------------------------------------------------
    //
    // Three of the four states in the README are rendered rather than hidden.
    // The fourth — muralis not installed — never gets this far, because
    // StartupCheck.qml blocks activation.

    StyledRect {
        width: parent.width
        height: visible ? bannerColumn.implicitHeight + Theme.spacingM * 2 : 0
        visible: bannerTitle.text !== ""
        radius: Theme.cornerRadius
        color: Theme.withAlpha(bannerIcon.color, 0.12)

        Row {
            anchors.fill: parent
            anchors.margins: Theme.spacingM
            spacing: Theme.spacingS

            DankIcon {
                id: bannerIcon
                anchors.verticalCenter: parent.verticalCenter
                size: Theme.iconSize - 2
                name: {
                    if (!popout.widget || !popout.widget.daemonUp)
                        return "cloud_off"
                    if (popout.widget.libraryEmpty)
                        return "photo_library"
                    return "info"
                }
                color: {
                    if (!popout.widget || !popout.widget.daemonUp)
                        return Theme.error
                    if (popout.widget.libraryEmpty)
                        return Theme.surfaceVariantText
                    return Theme.warning
                }
            }

            Column {
                id: bannerColumn
                width: parent.width - bannerIcon.width - Theme.spacingS
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2

                StyledText {
                    id: bannerTitle
                    width: parent.width
                    wrapMode: Text.WordWrap
                    font.pixelSize: Theme.fontSizeMedium
                    font.weight: Font.DemiBold
                    color: Theme.surfaceText
                    text: {
                        if (!popout.widget)
                            return ""
                        if (!popout.widget.daemonUp)
                            return "muralis is not running"
                        if (popout.widget.libraryEmpty)
                            return "The library is empty"
                        if (popout.widget.nothingApplied)
                            return "No wallpaper applied this session"
                        return ""
                    }
                }

                StyledText {
                    width: parent.width
                    wrapMode: Text.WordWrap
                    font.pixelSize: Theme.fontSizeSmall
                    color: Theme.surfaceVariantText
                    visible: text !== ""
                    text: {
                        if (!popout.widget)
                            return ""
                        if (!popout.widget.daemonUp)
                            return "The library lives behind the same socket, so there is nothing to show until the daemon is back. Start it with muralis-daemon."
                        if (popout.widget.libraryEmpty)
                            return "Find wallpapers with muralis search, then keep one with muralis favorites add."
                        if (popout.widget.nothingApplied)
                            return popout.widget.lastError !== ""
                            ? "The last apply failed: " + popout.widget.lastError
                            : "The daemon has applied nothing so far. Picking one below fixes it."
                        return ""
                    }
                }
            }
        }
    }

    // --- Library grid -----------------------------------------------------

    Grid {
        width: parent.width
        columns: popout.columns
        spacing: Theme.spacingS
        visible: popout.widget && popout.widget.daemonUp && popout.widget.library.length > 0
        // Dimmed rather than emptied while a page is in flight: a grid that
        // blanks on every page turn reads as a failure.
        opacity: popout.widget && popout.widget.libraryLoading ? 0.5 : 1

        Repeater {
            model: popout.widget ? popout.widget.library : []

            StyledRect {
                id: cell
                width: popout.cellWidth
                height: popout.cellHeight
                radius: Theme.cornerRadius
                color: Theme.surfaceContainerHigh
                clip: true

                required property var modelData
                readonly property var wallpaper: modelData
                property bool isCurrent: popout.widget && wallpaper
                                         && wallpaper.id === popout.widget.currentId

                CachingImage {
                    anchors.fill: parent
                    anchors.margins: cell.isCurrent ? 3 : 0
                    imagePath: popout.widget && cell.wallpaper
                               ? popout.widget.thumbnailFor(cell.wallpaper.id) : ""
                    fillMode: Image.PreserveAspectCrop
                    // Thumbnail grids decode enough as it is.
                    animate: false
                }

                // The selection ring is drawn over the image rather than as a
                // border, so the current wallpaper reads at a glance.
                Rectangle {
                    anchors.fill: parent
                    radius: parent.radius
                    color: "transparent"
                    border.width: cell.isCurrent ? 3 : 0
                    border.color: Theme.primary
                    visible: cell.isCurrent
                }

                StateLayer {
                    cornerRadius: parent.radius
                    tooltipText: cell.wallpaper
                                 ? cell.wallpaper.source_type + " · " + cell.wallpaper.width
                                 + "×" + cell.wallpaper.height
                                 : null
                    onClicked: {
                        if (popout.widget && cell.wallpaper)
                            popout.widget.setWallpaperById(cell.wallpaper.id)
                    }
                }
            }
        }
    }

    // --- Paging -----------------------------------------------------------

    Item {
        width: parent.width
        height: visible ? 36 : 0
        visible: popout.widget && popout.widget.daemonUp && popout.widget.libraryTotal > 0

        StyledText {
            anchors.left: parent.left
            anchors.leftMargin: Theme.spacingXS
            anchors.verticalCenter: parent.verticalCenter
            font.pixelSize: Theme.fontSizeSmall
            color: Theme.surfaceVariantText
            text: {
                if (!popout.widget || popout.widget.library.length === 0)
                    return ""
                const first = popout.widget.libraryOffset + 1
                const last = popout.widget.libraryOffset + popout.widget.library.length
                return first + "–" + last + " of " + popout.widget.libraryTotal
            }
        }

        Row {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Theme.spacingXS

            DankActionButton {
                iconName: "chevron_left"
                iconSize: Theme.iconSize - 4
                enabled: popout.widget && popout.widget.hasPrevPage
                opacity: enabled ? 1 : 0.35
                tooltipText: "Previous page"
                onClicked: {
                    if (popout.widget)
                        popout.widget.prevPage()
                }
            }

            DankActionButton {
                iconName: "chevron_right"
                iconSize: Theme.iconSize - 4
                enabled: popout.widget && popout.widget.hasNextPage
                opacity: enabled ? 1 : 0.35
                tooltipText: "Next page"
                onClicked: {
                    if (popout.widget)
                        popout.widget.nextPage()
                }
            }
        }
    }

    // --- Transport --------------------------------------------------------

    Item {
        width: parent.width
        height: visible ? 40 : 0
        visible: popout.widget && popout.widget.daemonUp

        Row {
            anchors.centerIn: parent
            spacing: Theme.spacingS

            DankActionButton {
                iconName: "skip_previous"
                tooltipText: "Previous wallpaper"
                onClicked: {
                    if (popout.widget)
                        popout.widget.prev()
                }
            }

            DankActionButton {
                iconName: popout.widget && popout.widget.paused ? "play_arrow" : "pause"
                backgroundColor: Theme.primary
                iconColor: Theme.onPrimary
                tooltipText: popout.widget && popout.widget.paused
                             ? "Resume rotation" : "Pause rotation"
                onClicked: {
                    if (popout.widget)
                        popout.widget.togglePause()
                }
            }

            DankActionButton {
                iconName: "skip_next"
                tooltipText: "Next wallpaper"
                onClicked: {
                    if (popout.widget)
                        popout.widget.next()
                }
            }
        }
    }

    // --- Mode switcher ----------------------------------------------------
    //
    // Every mode is shown; the ones this config cannot run are dimmed with the
    // reason, rather than hidden or offered as equals. `available_modes` is the
    // daemon's word on which is which — the missing precondition is always
    // "nothing declared in config.toml", so the prose is ours to write.

    Flow {
        width: parent.width
        spacing: Theme.spacingXS
        visible: popout.widget && popout.widget.daemonUp

        Repeater {
            model: popout.widget ? popout.widget.modeOrder : []

            StyledRect {
                id: chip

                required property var modelData
                readonly property string modeValue: modelData
                property bool usable: popout.widget
                                      && popout.widget.availableModes.indexOf(modeValue) >= 0
                property bool isCurrent: popout.widget && popout.widget.mode === modeValue

                width: chipRow.implicitWidth + Theme.spacingM * 2
                height: 30
                radius: height / 2
                color: isCurrent ? Theme.primarySelected : Theme.surfaceContainerHigh
                opacity: usable ? 1 : 0.4

                Row {
                    id: chipRow
                    anchors.centerIn: parent
                    spacing: Theme.spacingXS

                    DankIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        name: popout.widget ? popout.widget.modeIcon(chip.modeValue) : "wallpaper"
                        size: Theme.fontSizeMedium
                        color: chip.isCurrent ? Theme.primary : Theme.surfaceVariantText
                    }

                    StyledText {
                        anchors.verticalCenter: parent.verticalCenter
                        text: popout.widget ? popout.widget.modeLabel(chip.modeValue) : ""
                        font.pixelSize: Theme.fontSizeSmall
                        font.weight: chip.isCurrent ? Font.DemiBold : Font.Normal
                        color: chip.isCurrent ? Theme.primary : Theme.surfaceText
                    }
                }

                StateLayer {
                    cornerRadius: parent.radius
                    disabled: !chip.usable
                    tooltipText: chip.usable
                                 ? null
                                 : "Nothing declared in config.toml for this mode"
                    onClicked: {
                        if (chip.usable && !chip.isCurrent && popout.widget)
                            popout.widget.setMode(chip.modeValue)
                    }
                }
            }
        }
    }

    // --- Last error -------------------------------------------------------
    //
    // Shown on its own once something *is* applied: the banner above only
    // covers the nothing-applied case, and a failure that happened mid-session
    // is still worth saying out loud.

    StyledText {
        width: parent.width
        wrapMode: Text.WordWrap
        font.pixelSize: Theme.fontSizeSmall
        color: Theme.error
        visible: popout.widget && popout.widget.daemonUp
                 && popout.widget.lastError !== "" && !popout.widget.nothingApplied
        text: popout.widget ? "Last apply failed: " + popout.widget.lastError : ""
    }
}
