import QtQuick
import Quickshell.Io

// The only real gate on this widget loading.
//
// `dependencies` in plugin.json gates nothing — the schema calls it registry
// metadata and nothing in PluginService enforces it — so the binary check
// happens here. DMS calls `check(done)` and treats a null result as pass; a
// string or {title, details} fails activation and raises a toast.
QtObject {
    id: root

    property var _done: null

    property Process probe: Process {
        command: ["sh", "-c", "command -v muralis >/dev/null 2>&1"]
        running: false

        onExited: exitCode => {
            const done = root._done
            root._done = null
            if (!done)
                return
            if (exitCode === 0) {
                done(null)
                return
            }
            done({
                "title": "muralis is not installed",
                "details": "This widget drives the muralis daemon over its socket "
                    + "at /tmp/muralis-$UID.sock. Install muralis, start "
                    + "muralis-daemon, then enable the widget again."
            })
        }
    }

    function check(done) {
        root._done = done
        probe.running = true
    }
}
