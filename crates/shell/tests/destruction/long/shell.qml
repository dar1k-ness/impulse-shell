import QtQuick
import Quickshell
import Quickshell.Io
Scope {
    id: root
    readonly property string dir: Quickshell.env("DESTR_DIR")
    FileView { id: file; path: root.dir + "/file"; blockWrites: false; atomicWrites: true; printErrors: false }
    QtObject {
        Component.onDestruction: {
            const t0 = Date.now()
            let n = 0
            while (Date.now() - t0 < 1000) n++
            file.setText("0123456789abcdef".repeat(1 << 19))
            console.log("destroyed child, busy for 1 s:", Date.now() - t0 >= 1000)
        }
    }
    QtObject { Component.onDestruction: console.log("destroyed second child") }
    Component.onDestruction: console.log("destroyed root")
    Timer { interval: 60; running: true; onTriggered: { console.log("quit"); Qt.exit(3) } }
}
