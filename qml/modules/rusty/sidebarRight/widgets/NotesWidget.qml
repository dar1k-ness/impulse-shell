import QtQuick
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.rusty.sidebarRight.todo

// Sidebar widget "notes": the to-do list on its own card. In a row of toggles the list is
// scaled down to the cell; in one or two cells, where no list fits, it is the number of
// unfinished tasks, as high as the toggles.
SidebarCard {
    id: root
    property int cardHeight: 350
    property real padding: 10
    readonly property bool countOnly: root.inGrid && root.cellSize <= 2
    // The smallest width the list's two tabs fit in.
    readonly property real listMinWidth: 290
    readonly property real listWidth: Math.max(1, root.width - root.padding * 2)
    readonly property real listScale: root.inGrid ? Math.min(1, (root.cellsWidth - root.padding * 2) / root.listMinWidth) : 1
    readonly property int unfinished: Todo.list.filter(item => !item.done).length
    implicitWidth: root.inGrid ? root.cellsWidth : 0
    implicitHeight: root.countOnly ? root.baseCellHeight : Math.round(root.cardHeight * root.listScale)

    TodoWidget {
        visible: !root.countOnly
        x: root.padding
        y: root.padding
        width: root.listWidth / root.listScale
        height: Math.max(1, root.height - root.padding * 2) / root.listScale
        scale: root.listScale
        transformOrigin: Item.TopLeft
    }

    Row {
        id: count
        visible: root.countOnly
        anchors.centerIn: parent
        spacing: 6
        scale: Math.min(1, (root.width - 16) / Math.max(1, count.implicitWidth), (root.height - 12) / Math.max(1, count.implicitHeight))

        MaterialSymbol {
            anchors.verticalCenter: parent.verticalCenter
            text: "checklist"
            iconSize: Appearance.font.pixelSize.hugeass
            color: Appearance.colors.colOnLayer2
        }
        StyledText {
            anchors.verticalCenter: parent.verticalCenter
            text: root.unfinished
            font.pixelSize: Appearance.font.pixelSize.hugeass
            font.weight: Font.Medium
            color: Appearance.colors.colOnLayer2
        }
    }
}
