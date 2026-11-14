import QtQuick
import Quickshell
import Quickshell.Io
Scope {
    id: root
    readonly property string dir: Quickshell.env("DESTR_DIR")
    property bool up: false
    readonly property bool allUp: up && Single.up
    onAllUpChanged: console.log("ready", Single.name)
    Process {
        id: proc
        running: true
        command: ["sh", "-c", "echo $$ > " + root.dir + "/scope.pid; echo up; exec sleep infinity"]
        stdout: SplitParser { onRead: root.up = true }
    }
    // As the original's CliphistImage: the handler removes the decoded image.
    Component.onDestruction: {
        if (proc.running) proc.running = false
        Quickshell.execDetached(["rm", root.dir + "/image.png"])
        console.log("destroyed root")
    }
}
