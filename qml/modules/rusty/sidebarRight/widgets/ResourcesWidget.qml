import QtQuick
import ri.modules.common
import ri.modules.rusty.bar as Bar

// Sidebar widget "resources": RAM, swap and CPU usage gauges on their own card. In a row of
// toggles it is as high as they are: the gauges' rings alone, scaled down to the cell.
SidebarCard {
    id: root
    property real padding: root.inGrid ? 8 : 14
    implicitWidth: root.inGrid ? root.cellsWidth : 0
    implicitHeight: root.inGrid ? root.baseCellHeight : card.implicitHeight + root.padding * 2

    Bar.ResourcesCard {
        id: card
        anchors.centerIn: parent
        ringsOnly: root.inGrid
        colOnCard: root.inGrid ? Appearance.colors.colOnLayer2 : Appearance.colors.colOnLayer1
        // A narrow block keeps the gauges inside it.
        scale: Math.min(1, (root.width - root.padding * 2) / Math.max(1, card.implicitWidth), root.inGrid ? (root.height - root.padding * 2) / Math.max(1, card.implicitHeight) : 1)
    }
}
