import QtQuick
import ri.modules.rusty.bar as Bar

Bar.SysTray {
    vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    property string edge: parent?.edge ?? "top"
    readonly property bool shown: vertical || shortened === 0
    visible: shown
    invertSide: edge === "bottom" || edge === "right"
}
