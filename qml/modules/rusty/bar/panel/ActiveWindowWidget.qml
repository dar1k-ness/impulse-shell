import QtQuick
import ri.modules.common
import ri.modules.rusty.bar as Bar

Item {
    id: root
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    readonly property bool shown: !vertical && shortened === 0
    // Its title is cut with an ellipsis: it gives way down to this.
    readonly property real leastLength: 80
    visible: shown
    implicitWidth: window.implicitWidth + 10 + Appearance.rounding.screenRounding
    implicitHeight: Appearance.sizes.baseBarHeight

    Bar.ActiveWindow {
        id: window
        anchors {
            fill: parent
            leftMargin: 10
            rightMargin: Appearance.rounding.screenRounding
        }
    }
}
