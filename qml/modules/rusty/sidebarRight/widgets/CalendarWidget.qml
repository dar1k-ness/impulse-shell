import QtQuick
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.rusty.sidebarRight.calendar

// Sidebar widget "calendar": the month calendar on its own card. In a row of toggles the month
// is scaled down to the cell; in one or two cells, where no month is legible, it is today's
// date, as high as the toggles.
SidebarCard {
    id: root
    readonly property bool dateOnly: root.inGrid && root.cellSize <= 2
    readonly property real monthScale: root.inGrid ? Math.min(1, root.cellsWidth / Math.max(1, content.width)) : 1
    implicitWidth: root.inGrid ? root.cellsWidth : 0
    implicitHeight: root.dateOnly ? root.baseCellHeight : content.implicitHeight * root.monthScale

    // The month is laid out at its own size and scaled about its top.
    CalendarWidget {
        id: content
        visible: !root.dateOnly
        anchors.horizontalCenter: parent.horizontalCenter
        scale: root.monthScale
        transformOrigin: Item.Top
    }

    Row {
        id: today
        visible: root.dateOnly
        anchors.centerIn: parent
        spacing: 8
        scale: Math.min(1, (root.width - 16) / Math.max(1, today.implicitWidth), (root.height - 12) / Math.max(1, today.implicitHeight))

        StyledText {
            anchors.verticalCenter: parent.verticalCenter
            text: Qt.locale().toString(DateTime.clock.date, "d")
            font.pixelSize: Appearance.font.pixelSize.hugeass * 1.4
            font.weight: Font.Medium
            color: Appearance.colors.colOnLayer2
        }
        Column {
            visible: root.cellSize >= 2
            anchors.verticalCenter: parent.verticalCenter

            StyledText {
                text: Qt.locale().toString(DateTime.clock.date, "ddd")
                font.pixelSize: Appearance.font.pixelSize.small
                font.weight: Font.DemiBold
                color: Appearance.colors.colOnLayer2
            }
            StyledText {
                text: Qt.locale().toString(DateTime.clock.date, "MMM")
                font.pixelSize: Appearance.font.pixelSize.smaller
                color: Appearance.colors.colSubtext
            }
        }
    }
}
