import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

/**
 * Tabs inside a settings page: a grid of icon + label chips, four per row, so long labels
 * (ru "Экран блокировки") keep their full text and equal gaps.
 * tabs: [{ name, icon }]
 */
Item {
    id: root
    property var tabs: []
    property int currentIndex: 0
    signal tabClicked(int index)

    readonly property int columns: 4
    readonly property real gap: 8
    readonly property real chipHeight: 40
    readonly property real cellWidth: Math.floor((width - gap * (columns - 1)) / columns)
    readonly property int rows: Math.ceil(tabs.length / columns)

    Layout.fillWidth: true
    implicitHeight: rows * chipHeight + (rows - 1) * gap

    Repeater {
        model: root.tabs
        delegate: Rectangle {
            id: tab
            required property var modelData
            required property int index
            readonly property bool current: index === root.currentIndex
            x: (index % root.columns) * (root.cellWidth + root.gap)
            y: Math.floor(index / root.columns) * (root.chipHeight + root.gap)
            width: root.cellWidth
            height: root.chipHeight
            radius: height / 2
            color: tab.current ? Appearance.colors.colSecondaryContainer : tabMouse.containsMouse ? Appearance.colors.colLayer2Hover : Appearance.colors.colLayer2

            RowLayout {
                anchors.centerIn: parent
                width: Math.min(implicitWidth, tab.width - 16)
                spacing: 6
                MaterialSymbol {
                    text: tab.modelData.icon
                    iconSize: 20
                    fill: tab.current ? 1 : 0
                    color: tab.current ? Appearance.colors.colOnSecondaryContainer : Appearance.colors.colOnLayer2
                }
                StyledText {
                    Layout.fillWidth: true
                    text: tab.modelData.name
                    elide: Text.ElideRight
                    font.pixelSize: Appearance.font.pixelSize.smallie
                    font.weight: tab.current ? Font.DemiBold : Font.Medium
                    color: tab.current ? Appearance.colors.colOnSecondaryContainer : Appearance.colors.colOnLayer2
                }
            }
            MouseArea {
                id: tabMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.tabClicked(tab.index)
            }
        }
    }
}
