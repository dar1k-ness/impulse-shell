pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import Rustshell
import Rustshell.Hyprland
import ri.modules.common
import ri.modules.common.functions
import ri.modules.waffle.looks

Rectangle {
    id: root

    required property var target
    z: 0

    anchors {
        fill: target
        margins: -border.width
    }

    border.color: Looks.colors.ambientShadow
    border.width: 1
    color: "transparent"
    radius: target.radius + border.width
}
