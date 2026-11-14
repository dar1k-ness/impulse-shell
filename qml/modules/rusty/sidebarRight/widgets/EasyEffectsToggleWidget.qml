import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "easyEffects": the sidebar container sets the cell span and metrics.
AndroidEasyEffectsToggle {
    buttonIndex: -1
    buttonData: ({ "type": "easyEffects", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
