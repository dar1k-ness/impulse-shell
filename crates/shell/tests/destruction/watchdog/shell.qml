import QtQuick
import Quickshell
import Quickshell.Io
Scope {
    id: root
    readonly property string dir: Quickshell.env("DESTR_DIR")
    FileView { id: file; path: root.dir + "/file"; blockWrites: false; atomicWrites: true; printErrors: false }
    QtObject {
        Component.onDestruction: {
            console.log("destroyed child, never returns")
            while (true) {}
        }
    }
    QtObject {
        Component.onDestruction: {
            file.setText("written")
            console.log("destroyed second child")
        }
    }
    Component.onDestruction: console.log("destroyed root")
    Timer { interval: 60; running: true; onTriggered: { console.log("quit"); Qt.exit(3) } }
}
