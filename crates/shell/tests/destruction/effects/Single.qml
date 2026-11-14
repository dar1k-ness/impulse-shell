pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Io
Singleton {
    id: root
    property string name: "single"
    property bool up: false
    // As the original's AcpSession: a Process of a singleton that its handler would stop.
    Process {
        id: proc
        running: true
        command: ["sh", "-c", "echo $$ > " + Quickshell.env("DESTR_DIR") + "/single.pid; echo up; exec sleep infinity"]
        stdout: SplitParser { onRead: root.up = true }
    }
    Component.onDestruction: {
        if (proc.running) proc.running = false
        Quickshell.execDetached(["sh", "-c", "echo singleton > " + Quickshell.env("DESTR_DIR") + "/singleton"])
        console.log("destroyed singleton")
    }
}
