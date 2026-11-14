import QtQuick
import Quickshell
import Quickshell.Io
Scope {
    id: root
    readonly property string dir: Quickshell.env("DESTR_DIR")
    Process {
        id: started
        command: ["sh", "-c", "echo $$ > " + root.dir + "/started.pid; exec sleep infinity"]
    }
    Process {
        id: stopped
        running: true
        command: ["sh", "-c", "echo $$ > " + root.dir + "/stopped.pid; echo up; exec sleep infinity"]
        stdout: SplitParser { onRead: root.up = true }
    }
    property bool up: false
    readonly property bool allUp: up && Single.up
    onAllUpChanged: { console.log("quit", Single.name); Qt.quit() }
    FileView { id: file; path: root.dir + "/file"; blockWrites: false; atomicWrites: true; printErrors: false }
    Timer { id: timer; interval: 1; onTriggered: console.log("timer started by the handler") }
    Component.onDestruction: {
        Quickshell.execDetached(["sh", "-c", "echo detached > " + root.dir + "/detached"])
        started.running = true
        stopped.running = false
        file.setText("written")
        timer.start()
        console.log("destroyed root")
    }
}
