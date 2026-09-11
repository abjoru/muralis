pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import qs.Common
import qs.Widgets
import qs.Modules.Plugins

// The muralis Consumer: a DankBar pill plus a popout over the Library.
//
// Everything here speaks the daemon socket directly (ADR 0002). Two sockets,
// because the IPC contract has two shapes:
//
//   eventSocket   — one Subscribe, held open forever. It carries the snapshot
//                   on connect and every change after, which is what keeps the
//                   matugen palette matching the wallpaper through rotations
//                   this widget did not initiate.
//   commandSocket — one-shot requests. The daemon answers a command and hangs
//                   up, so each one gets a fresh dial; they queue and drain one
//                   at a time rather than racing each other onto a dead socket.
PluginComponent {
    id: root

    // --- Daemon addresses -------------------------------------------------

    // Matches MuralisPaths::socket_path(): /tmp/muralis-$UID.sock. The uid is
    // the last segment of XDG_RUNTIME_DIR (/run/user/1000), with `id -u` as a
    // fallback for a session that somehow lacks it — a widget that silently
    // never connects is the failure mode worth spending ten lines to avoid.
    property string uid: {
        const runtime = Quickshell.env("XDG_RUNTIME_DIR") || ""
        const tail = runtime.split("/").filter(s => s.length > 0).pop() || ""
        return /^[0-9]+$/.test(tail) ? tail : ""
    }
    readonly property string socketPath: uid ? "/tmp/muralis-" + uid + ".sock" : ""

    // Matches MuralisPaths' cache dir; thumbnails are read off disk rather
    // than through the daemon (README: one file per Wallpaper, <id>_thumb.jpg).
    readonly property string thumbnailDir: (Quickshell.env("XDG_CACHE_HOME")
        || (Quickshell.env("HOME") + "/.cache")) + "/muralis/thumbnails"

    function thumbnailFor(id) {
        return id ? "file://" + thumbnailDir + "/" + id + "_thumb.jpg" : ""
    }

    Process {
        command: ["id", "-u"]
        running: root.uid === ""
        stdout: SplitParser {
            onRead: data => {
                const value = data.trim()
                if (/^[0-9]+$/.test(value))
                    root.uid = value
            }
        }
    }

    // --- Daemon state -----------------------------------------------------

    // Fed by the subscription. `currentWallpaper` is the whole Wallpaper row,
    // so the grid, the pill and SessionData all read one source.
    property var currentWallpaper: null
    property string mode: ""
    property bool paused: false

    // Fed by Status: the facts events do not carry.
    property var availableModes: []
    property int wallpaperCount: 0
    property string lastError: ""

    readonly property bool daemonUp: eventSocket.linkUp
    readonly property string currentId: currentWallpaper ? currentWallpaper.id : ""
    // The daemon is up but has nothing on screen — a real state, not an empty
    // one (see #11). Saying "nothing selected" here reads as a broken widget.
    readonly property bool nothingApplied: daemonUp && !currentWallpaper
    readonly property bool libraryEmpty: daemonUp && wallpaperCount === 0

    // --- Library page -----------------------------------------------------

    readonly property int pageSize: 16
    property var library: []
    property int libraryTotal: 0
    property int libraryOffset: 0
    property bool libraryLoading: false
    readonly property bool hasPrevPage: libraryOffset > 0
    readonly property bool hasNextPage: libraryOffset + library.length < libraryTotal

    popoutWidth: 440
    popoutHeight: 0

    // --- Commands ---------------------------------------------------------

    function setWallpaperById(id) {
        _request({command: "set_wallpaper", id: id}, () => refreshStatus())
    }

    function next() {
        _request({command: "next"}, () => refreshStatus())
    }

    function prev() {
        _request({command: "prev"}, () => refreshStatus())
    }

    function togglePause() {
        _request({command: paused ? "resume" : "pause"}, () => refreshStatus())
    }

    function setMode(value) {
        _request({command: "set_mode", mode: value}, response => {
            // A refused mode is the daemon telling us the config cannot run it.
            // Surface it rather than leaving the chip looking stuck.
            if (response && response.status === "error")
                root.lastError = response.message || ""
            refreshStatus()
        })
    }

    function refreshStatus() {
        _request({command: "status"}, response => {
            if (!response || response.status !== "ok" || !response.data)
                return
            const data = response.data
            root.mode = data.mode || ""
            root.paused = data.paused === true
            root.wallpaperCount = data.wallpaper_count || 0
            root.availableModes = data.available_modes || []
            root.lastError = data.last_error || ""
            // Status carries only the id; the row itself arrives on the
            // subscription. Clearing here keeps the two from disagreeing when
            // the daemon has nothing applied.
            if (!data.current_wallpaper)
                root.currentWallpaper = null
            // A page load already in flight will answer this; without the
            // guard, every connect fetches page 0 twice.
            if (root.libraryTotal !== root.wallpaperCount && !root.libraryLoading)
                loadPage(0)
        })
    }

    function loadPage(offset) {
        if (!daemonUp)
            return
        libraryLoading = true
        _request({command: "favorites", offset: offset, limit: pageSize}, response => {
            root.libraryLoading = false
            if (!response || response.status !== "ok" || !response.data)
                return
            root.library = response.data.wallpapers || []
            root.libraryTotal = response.data.total || 0
            root.libraryOffset = response.data.offset || 0
        })
    }

    function nextPage() {
        if (hasNextPage)
            loadPage(libraryOffset + pageSize)
    }

    function prevPage() {
        if (hasPrevPage)
            loadPage(Math.max(0, libraryOffset - pageSize))
    }

    function refresh() {
        refreshStatus()
        loadPage(libraryOffset)
    }

    // --- Subscription -----------------------------------------------------

    DankSocket {
        id: eventSocket
        path: root.socketPath
        connected: root.socketPath !== ""

        onConnectionStateChanged: {
            if (linkUp) {
                // The daemon answers Subscribe with a snapshot, so this is also
                // how the widget catches up after a redial.
                send({command: "subscribe"})
                root.refresh()
            } else {
                root.library = []
                root.libraryTotal = 0
            }
        }

        parser: SplitParser {
            onRead: line => root._onEvent(line)
        }
    }

    function _onEvent(line) {
        const text = (line || "").trim()
        if (!text)
            return
        let event = null
        try {
            event = JSON.parse(text)
        } catch (e) {
            return
        }
        if (!event || !event.event)
            return

        switch (event.event) {
        case "wallpaper_changed":
            _adoptWallpaper(event.wallpaper)
            break
        case "mode_changed":
            root.mode = event.mode || ""
            break
        case "pause_changed":
            root.paused = event.paused === true
            break
        }
        // Unknown event kinds are ignored on purpose: the contract says adding
        // one is backwards-compatible, so a newer daemon must not break us.
    }

    function _adoptWallpaper(wallpaper) {
        if (!wallpaper || !wallpaper.file_path)
            return
        root.currentWallpaper = wallpaper
        root.lastError = ""
        // The point of the whole exercise: this regenerates DMS's matugen
        // palette from the image. Guarded, because every redial replays the
        // snapshot and regenerating a palette we already have is pure cost.
        if (SessionData.wallpaperPath !== wallpaper.file_path)
            SessionData.setWallpaper(wallpaper.file_path)
    }

    // --- One-shot commands ------------------------------------------------

    // The daemon hangs up after answering, so a command dials, sends, reads one
    // line and drops the socket. Queued rather than concurrent: a fresh dial
    // per command keeps the reply unambiguous, and commands here are always
    // user-initiated, never hot.
    property var _queue: []
    property var _inFlight: null

    function _request(request, handler) {
        _queue.push({request: request, handler: handler})
        _pump()
    }

    function _pump() {
        if (_inFlight || _queue.length === 0 || socketPath === "")
            return
        _inFlight = _queue.shift()
        commandTimeout.restart()
        commandSocket.connected = true
    }

    function _finish(response) {
        const entry = _inFlight
        _inFlight = null
        commandTimeout.stop()
        commandSocket.connected = false
        if (entry && entry.handler)
            entry.handler(response)
        Qt.callLater(_pump)
    }

    DankSocket {
        id: commandSocket
        path: root.socketPath
        connected: false
        reconnectBaseMs: 100

        onConnectionStateChanged: {
            if (linkUp && root._inFlight)
                send(root._inFlight.request)
        }

        parser: SplitParser {
            onRead: line => {
                const text = (line || "").trim()
                if (!text)
                    return
                let response = null
                try {
                    response = JSON.parse(text)
                } catch (e) {
                    response = null
                }
                root._finish(response)
            }
        }
    }

    // A command that never comes back must not wedge the queue behind it.
    Timer {
        id: commandTimeout
        interval: 5000
        repeat: false
        onTriggered: {
            if (root._inFlight)
                root._finish(null)
        }
    }

    // --- Presentation helpers ---------------------------------------------

    readonly property var modeOrder: ["static", "random", "random_startup",
                                      "sequential", "workspace", "schedule"]

    function modeLabel(value) {
        switch (value) {
        case "static": return "Static"
        case "random": return "Random"
        case "random_startup": return "At startup"
        case "sequential": return "Sequential"
        case "workspace": return "Per workspace"
        case "schedule": return "Scheduled"
        }
        return value || "unknown"
    }

    function modeIcon(value) {
        switch (value) {
        case "static": return "image"
        case "random": return "shuffle"
        case "random_startup": return "bolt"
        case "sequential": return "repeat"
        case "workspace": return "grid_view"
        case "schedule": return "schedule"
        }
        return "wallpaper"
    }

    readonly property string pillIcon: {
        if (!daemonUp)
            return "cloud_off"
        if (nothingApplied)
            return "image_not_supported"
        return modeIcon(mode)
    }

    readonly property color pillColor: {
        if (!daemonUp)
            return Theme.error
        if (nothingApplied)
            return Theme.warning
        return Theme.surfaceText
    }

    readonly property string pillText: {
        if (!daemonUp)
            return "not running"
        if (nothingApplied)
            return "none"
        return modeLabel(mode)
    }

    // --- Pills ------------------------------------------------------------

    horizontalBarPill: Component {
        Row {
            spacing: Theme.spacingXS

            DankCircularImage {
                width: root.iconSize
                height: root.iconSize
                anchors.verticalCenter: parent.verticalCenter
                visible: root.currentId !== ""
                imageSource: root.thumbnailFor(root.currentId)
                fallbackIcon: "wallpaper"
            }

            DankIcon {
                anchors.verticalCenter: parent.verticalCenter
                visible: root.currentId === ""
                name: root.pillIcon
                size: root.iconSize
                color: root.pillColor
            }

            DankIcon {
                anchors.verticalCenter: parent.verticalCenter
                visible: root.daemonUp && root.paused
                name: "pause"
                size: root.iconSize - 4
                color: Theme.surfaceVariantText
            }

            StyledText {
                anchors.verticalCenter: parent.verticalCenter
                text: root.pillText
                font.pixelSize: Theme.fontSizeSmall
                color: root.pillColor
            }
        }
    }

    verticalBarPill: Component {
        Column {
            spacing: 2

            DankCircularImage {
                width: root.iconSize
                height: root.iconSize
                anchors.horizontalCenter: parent.horizontalCenter
                visible: root.currentId !== ""
                imageSource: root.thumbnailFor(root.currentId)
                fallbackIcon: "wallpaper"
            }

            DankIcon {
                anchors.horizontalCenter: parent.horizontalCenter
                visible: root.currentId === ""
                name: root.pillIcon
                size: root.iconSize
                color: root.pillColor
            }

            DankIcon {
                anchors.horizontalCenter: parent.horizontalCenter
                visible: root.daemonUp && root.paused
                name: "pause"
                size: root.iconSize - 6
                color: Theme.surfaceVariantText
            }
        }
    }

    // --- Popout -----------------------------------------------------------

    popoutContent: Component {
        MuralisPopout {
            widget: root
        }
    }
}
