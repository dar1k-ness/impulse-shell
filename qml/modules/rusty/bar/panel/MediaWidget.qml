import QtQuick
import ri.modules.rusty.bar as Bar
import ri.modules.rusty.verticalBar as VBar

Loader {
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    property real fixedLength: parent?.fixedLength ?? -1
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    readonly property bool shown: vertical || shortened < 2
    visible: shown
    sourceComponent: vertical ? verticalMedia : horizontalMedia

    Component {
        id: horizontalMedia
        // Fixed shorter than a few letters of the title take: the progress ring alone.
        Bar.Media {
            showTitle: fixedLength < 0 || fixedLength >= 72
        }
    }
    Component {
        id: verticalMedia
        VBar.VerticalMedia {}
    }
}
