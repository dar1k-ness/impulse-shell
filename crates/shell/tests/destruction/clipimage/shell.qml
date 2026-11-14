import QtQuick
import Quickshell
import Impulse

Scope {
    id: root
    readonly property string file: Quickshell.env("DESTR_DIR") + "/image.png"

    Component.onDestruction: {
        ClipboardHistory.removeImage(root.file)
        console.log("destroyed root")
    }

    Timer {
        interval: 60
        running: true
        onTriggered: {
            console.log("quit")
            Qt.quit()
        }
    }
}
