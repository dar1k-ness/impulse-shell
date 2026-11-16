import QtQuick
import ri
import ri.modules.common
import ri.modules.rusty.bar as Bar
import ri.modules.rusty.verticalBar as VBar

MouseArea {
    id: root
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    property real fixedLength: parent?.fixedLength ?? -1
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    property bool showDate: true
    implicitWidth: clock.implicitWidth
    implicitHeight: clock.implicitHeight
    // Stretching or pulled by an end, it gives way no further than what it shows takes.
    readonly property real leastLength: vertical ? implicitHeight : implicitWidth
    // Pulled by an end in edit mode, it stops with room left between what it shows and the
    // handles at its ends.
    readonly property real contentLength: leastLength + (vertical ? 0 : 20)
    // Shorter than that, as a layout may have it, its text reaches its ends: edit mode draws
    // its outline and handles around this length, and the clock stays as it is.
    readonly property real clearLength: contentLength
    onPressed: GlobalStates.sidebarRightOpen = !GlobalStates.sidebarRightOpen
    // Read by the run this clock is in (PanelRun).
    readonly property bool groupOpensRightSidebar: !vertical

    Loader {
        id: clock
        anchors.fill: parent
        sourceComponent: root.vertical ? verticalClock : horizontalClock
    }
    Component {
        id: horizontalClock
        Bar.ClockWidget {
            showDate: root.showDate && Config.options.bar.verbose && root.shortened < 2
            room: root.fixedLength
        }
    }
    Component {
        id: verticalClock
        VBar.VerticalClockWidget {}
    }
}
