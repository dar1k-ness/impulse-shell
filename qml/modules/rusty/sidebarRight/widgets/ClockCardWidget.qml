import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.widgets

// Sidebar widget "clock": the time, date and uptime on a block card, as the original's bar clock
// popup shows them. In a row of toggles it is as high as they are: the time, with the date beside
// it where there is room, scaled down to the cell.
SidebarCard {
    id: root
    // Setting (widgets.toml), shared with the panel and desktop clocks.
    property bool showDate: true

    implicitWidth: root.inGrid ? root.cellsWidth : content.implicitWidth + root.padding * 2
    implicitHeight: root.inGrid ? root.baseCellHeight : content.implicitHeight + root.padding * 2
    readonly property real padding: 16

    Row {
        id: compact
        visible: root.inGrid
        anchors.centerIn: parent
        spacing: 10
        scale: Math.min(1, (root.width - 20) / Math.max(1, compact.implicitWidth), (root.height - 12) / Math.max(1, compact.implicitHeight))

        StyledText {
            anchors.verticalCenter: parent.verticalCenter
            text: DateTime.time
            font.pixelSize: Appearance.font.pixelSize.hugeass * 1.4
            font.weight: Font.Medium
            color: Appearance.colors.colOnLayer2
        }
        StyledText {
            visible: root.showDate && root.cellSize >= 3
            anchors.verticalCenter: parent.verticalCenter
            text: DateTime.longDate
            font.pixelSize: Appearance.font.pixelSize.small
            color: Appearance.colors.colSubtext
        }
    }

    ColumnLayout {
        id: content
        visible: !root.inGrid
        anchors {
            left: parent.left
            right: parent.right
            verticalCenter: parent.verticalCenter
            margins: root.padding
        }
        spacing: 2

        StyledText {
            text: DateTime.time
            font.pixelSize: Appearance.font.pixelSize.hugeass * 2
            font.weight: Font.Medium
            color: Appearance.colors.colOnLayer1
        }
        StyledText {
            visible: root.showDate
            text: DateTime.longDate
            font.pixelSize: Appearance.font.pixelSize.normal
            color: Appearance.colors.colOnLayer1
        }
        RowLayout {
            spacing: 4
            MaterialSymbol {
                text: "timelapse"
                iconSize: Appearance.font.pixelSize.normal
                color: Appearance.colors.colSubtext
            }
            StyledText {
                text: Translation.tr("Up %1").arg(DateTime.uptime)
                font.pixelSize: Appearance.font.pixelSize.smaller
                color: Appearance.colors.colSubtext
            }
        }
    }
}
