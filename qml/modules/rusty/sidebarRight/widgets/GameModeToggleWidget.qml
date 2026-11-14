import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "gameMode": the sidebar container sets the cell span and metrics.
AndroidGameModeToggle {
    buttonIndex: -1
    buttonData: ({ "type": "gameMode", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
