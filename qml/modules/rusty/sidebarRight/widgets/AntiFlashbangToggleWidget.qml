import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "antiFlashbang": the sidebar container sets the cell span and metrics.
AndroidAntiFlashbangToggle {
    signal openNightLightDialog()
    onOpenMenu: openNightLightDialog()
    buttonIndex: -1
    buttonData: ({ "type": "antiFlashbang", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
