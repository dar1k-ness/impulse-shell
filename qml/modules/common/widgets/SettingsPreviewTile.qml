import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

/**
 * A selectable picture tile with a label under it (bar position, clock style, theme).
 */
ColumnLayout {
    id: root
    property string label: ""
    property string icon: ""
    property bool selected: false
    property real tileWidth: 118
    property real tileHeight: 72
    default property alias tileData: tileContent.data
    signal clicked

    spacing: 6

    Rectangle {
        id: frame
        Layout.alignment: Qt.AlignHCenter
        implicitWidth: root.tileWidth + 8
        implicitHeight: root.tileHeight + 8
        radius: Appearance.rounding.normal + 4
        color: "transparent"
        border.width: 2
        border.color: root.selected ? Appearance.colors.colPrimary : (tileMouse.containsMouse ? Appearance.colors.colOutlineVariant : "transparent")
        Behavior on border.color {
            animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
        }

        Item {
            id: tileContent
            anchors.centerIn: parent
            width: root.tileWidth
            height: root.tileHeight
            clip: true
            scale: tileMouse.pressed ? 0.96 : 1
            Behavior on scale {
                animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
            }
        }

        MouseArea {
            id: tileMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.clicked()
        }
    }

    RowLayout {
        Layout.alignment: Qt.AlignHCenter
        spacing: 4
        MaterialSymbol {
            visible: root.icon.length > 0
            text: root.icon
            iconSize: Appearance.font.pixelSize.normal
            color: root.selected ? Appearance.colors.colPrimary : Appearance.colors.colOnLayer2
        }
        StyledText {
            text: root.label
            font.pixelSize: Appearance.font.pixelSize.small
            color: root.selected ? Appearance.colors.colPrimary : Appearance.colors.colOnLayer2
        }
    }
}
