import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "bluetooth": the sidebar container sets the cell span and metrics.
AndroidBluetoothToggle {
    signal openBluetoothDialog()
    onOpenMenu: openBluetoothDialog()
    buttonIndex: -1
    buttonData: ({ "type": "bluetooth", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
