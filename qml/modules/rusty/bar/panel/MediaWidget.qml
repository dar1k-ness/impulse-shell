import QtQuick
import ri.modules.rusty.bar as Bar
import ri.modules.rusty.verticalBar as VBar

Loader {
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    readonly property bool shown: vertical || shortened < 2
    visible: shown
    sourceComponent: vertical ? verticalMedia : horizontalMedia

    Component {
        id: horizontalMedia
        Bar.Media {}
    }
    Component {
        id: verticalMedia
        VBar.VerticalMedia {}
    }
}
