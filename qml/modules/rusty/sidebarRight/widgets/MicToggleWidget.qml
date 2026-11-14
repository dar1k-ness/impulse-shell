import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "mic": the sidebar container sets the cell span and metrics.
AndroidMicToggle {
    signal openAudioInputDialog()
    onOpenMenu: openAudioInputDialog()
    buttonIndex: -1
    buttonData: ({ "type": "mic", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
