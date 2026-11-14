import QtQuick
import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

// Sidebar widget "cloudflareWarp": the sidebar container sets the cell span and metrics.
AndroidCloudflareWarpToggle {
    buttonIndex: -1
    buttonData: ({ "type": "cloudflareWarp", "size": cellSize })
    expandedSize: cellSize > 1
    cellSize: 1
    baseCellWidth: 0
    baseCellHeight: 56
    cellSpacing: 6
}
