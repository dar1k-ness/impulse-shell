// A Connections made before a binding first reads the property it listens to: Qt calls what
// is connected to the signal in the order it connected, the Connections before the binding.
import QtQuick
Item {
    id: root
    property int k: 1
    property bool on: false
    property int b: on ? k : -1
    onBChanged: console.log("binding " + b)
    Connections {
        target: root
        function onKChanged() { console.log("conn " + root.k) }
    }
    Timer {
        interval: 0; running: true
        onTriggered: {
            root.on = true
            console.log("--")
            root.k = 7
            Qt.quit()
        }
    }
}
