import QtQuick
import ri.modules.rusty.bar as Bar
import ri.modules.rusty.verticalBar as VBar

Loader {
    id: root
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    property real fixedLength: parent?.fixedLength ?? -1
    property string lengthKind: parent?.lengthKind ?? "auto"
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    readonly property bool shown: vertical || shortened < 2
    // Stretching, it gives way down to its progress ring: its title is cut with an ellipsis.
    readonly property real leastLength: 24
    // Pulled by an end in edit mode, it stops where its title still reads (its ring alone
    // where it shows no title), with room left between that and the handles at its ends.
    readonly property real contentLength: vertical ? leastLength : (item?.titledLength || ring) + 20
    readonly property real ring: 20
    // Of a set length, it keeps its ring and title that far from its ends, as far as the
    // length has room for beside the ring.
    readonly property real endRoom: !vertical && lengthKind === "px" ? Math.max(0, Math.min(10, (fixedLength - ring) / 2)) : 0
    visible: shown
    sourceComponent: vertical ? verticalMedia : horizontalMedia

    Component {
        id: horizontalMedia
        // Shorter than a few letters of the title take, or than a title of fewer (fixed so, or
        // stretching in a group that has no more for it): the progress ring alone.
        Bar.Media {
            endRoom: root.endRoom
            showTitle: lengthKind === "auto" || (lengthKind === "px" ? fixedLength - endRoom * 2 : width) >= Math.min(72, titledLength)
        }
    }
    Component {
        id: verticalMedia
        VBar.VerticalMedia {}
    }
}
