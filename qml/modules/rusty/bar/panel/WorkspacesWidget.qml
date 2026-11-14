import QtQuick
import ri
import ri.modules.rusty.bar as Bar

Bar.Workspaces {
    vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    readonly property real groupPadding: vertical ? 6 : 5
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight

    MouseArea {
        // Right-click to toggle overview
        anchors.fill: parent
        acceptedButtons: Qt.RightButton
        onPressed: event => {
            if (event.button === Qt.RightButton)
                GlobalStates.overviewOpen = !GlobalStates.overviewOpen;
        }
    }
}
