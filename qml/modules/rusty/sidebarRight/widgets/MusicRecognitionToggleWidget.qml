import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "musicRecognition": the sidebar container sets the cell span and metrics.
AndroidMusicRecognition {
    buttonIndex: -1
    buttonData: ({ "type": "musicRecognition", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
