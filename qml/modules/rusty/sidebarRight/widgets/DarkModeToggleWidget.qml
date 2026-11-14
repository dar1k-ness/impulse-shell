import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "darkMode": the sidebar container sets the cell span and metrics.
AndroidDarkModeToggle {
    buttonIndex: -1
    buttonData: ({ "type": "darkMode", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
