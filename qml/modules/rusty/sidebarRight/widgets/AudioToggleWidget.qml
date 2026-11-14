import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "audio": the sidebar container sets the cell span and metrics.
AndroidAudioToggle {
    signal openAudioOutputDialog()
    onOpenMenu: openAudioOutputDialog()
    buttonIndex: -1
    buttonData: ({ "type": "audio", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
