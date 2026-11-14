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
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    property bool showDate: true
    implicitWidth: clock.implicitWidth
    implicitHeight: clock.implicitHeight
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
        }
    }
    Component {
        id: verticalClock
        VBar.VerticalClockWidget {}
    }
}
