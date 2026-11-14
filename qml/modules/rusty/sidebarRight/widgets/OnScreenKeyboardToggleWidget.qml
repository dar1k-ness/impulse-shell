import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "onScreenKeyboard": the sidebar container sets the cell span and metrics.
AndroidOnScreenKeyboardToggle {
    buttonIndex: -1
    buttonData: ({ "type": "onScreenKeyboard", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
