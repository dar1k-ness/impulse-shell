import QtQuick
import Rustshell.Bluetooth
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets

QuickToggleModel {
    name: Translation.tr("Bluetooth")
    statusText: BluetoothStatus.firstActiveDevice?.name ?? Translation.tr("Not connected")
    tooltipText: Translation.tr("%1 | Right-click to configure").arg(
        (BluetoothStatus.firstActiveDevice?.name ?? Translation.tr("Bluetooth"))
        + (BluetoothStatus.activeDeviceCount > 1 ? ` +${BluetoothStatus.activeDeviceCount - 1}` : "")
    )
    icon: BluetoothStatus.connected ? "bluetooth_connected" : BluetoothStatus.enabled ? "bluetooth" : "bluetooth_disabled"

    available: BluetoothStatus.available
    toggled: BluetoothStatus.enabled
    mainAction: () => {
        Bluetooth.defaultAdapter.enabled = !Bluetooth.defaultAdapter?.enabled
    }
    hasMenu: true
}
