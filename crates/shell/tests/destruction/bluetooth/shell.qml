import QtQuick
import Quickshell
import Quickshell.Bluetooth

Scope {
    id: root
    readonly property bool on: Bluetooth.defaultAdapter?.enabled ?? false
    // The test makes the adapter discoverable once bluetoothd has been asked to discover.
    readonly property bool go: Bluetooth.defaultAdapter?.discoverable ?? false

    onOnChanged: if (on) {
        console.log("start discovery")
        Bluetooth.defaultAdapter.discovering = true
    }
    onGoChanged: if (go) {
        console.log("quit")
        Qt.quit()
    }

    Component.onDestruction: {
        if (Bluetooth.defaultAdapter) Bluetooth.defaultAdapter.discovering = false
        console.log("destroyed root")
    }
}
