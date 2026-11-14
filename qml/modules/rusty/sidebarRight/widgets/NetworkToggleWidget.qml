import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "network": the sidebar container sets the cell span and metrics.
AndroidNetworkToggle {
    signal openWifiDialog()
    onOpenMenu: openWifiDialog()
    buttonIndex: -1
    buttonData: ({ "type": "network", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
