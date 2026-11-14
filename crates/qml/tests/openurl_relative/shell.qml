import QtQuick
import Quickshell
// Quickshell runs the cases of Open.qml itself: `qs.sh` pairs what it logs with what
// `xdg-open` was given.
ShellRoot {
    Open { id: open }
    Timer {
        interval: 40; running: true; repeat: true
        property int at: 0
        onTriggered: {
            if (at >= open.cases.length * 3)
                return Qt.quit()
            console.log("CASE " + JSON.stringify(open.cases[Math.floor(at / 3)]) + "\t" + open.kinds[at % 3] + "\t" + open.go(at))
            at += 1
        }
    }
}
