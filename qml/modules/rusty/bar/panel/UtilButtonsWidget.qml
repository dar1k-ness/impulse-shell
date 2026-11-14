import QtQuick
import ri.modules.common
import ri.modules.rusty.bar as Bar

Bar.UtilButtons {
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    readonly property bool shown: !vertical && Config.options.bar.verbose && shortened === 0
    visible: shown
}
