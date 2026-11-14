import QtQuick
import Quickshell
import Quickshell.Bluetooth
import Impulse

Scope {
    id: root
    readonly property bool on: Bluetooth.defaultAdapter?.enabled ?? false
    // The test makes the adapter not pairable once bluetoothd holds its answers.
    readonly property bool go: !(Bluetooth.defaultAdapter?.pairable ?? true)

    onOnChanged: if (on) console.log("ready")
    onGoChanged: if (go) {
        console.log("quit")
        Qt.quit()
    }

    Component.onDestruction: {
        // bluetoothd holds the first write; the second waits behind it.
        Bluetooth.defaultAdapter.discoverable = true
        Bluetooth.defaultAdapter.pairable = true
        // The session bus never answers: the send waits for the connection.
        Notify.send("Exit", "queued")
        console.log("destroyed root")
    }
}
