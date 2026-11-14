import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "powerProfile": the sidebar container sets the cell span and metrics.
AndroidPowerProfileToggle {
    buttonIndex: -1
    buttonData: ({ "type": "powerProfile", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
