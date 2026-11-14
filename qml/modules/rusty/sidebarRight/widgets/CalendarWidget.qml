import QtQuick
import ri.modules.common
import ri.modules.rusty.sidebarRight.calendar

// Sidebar widget "calendar": the month calendar on its own card.
Rectangle {
    id: root
    radius: Appearance.rounding.normal
    color: Appearance.colors.colLayer1
    implicitHeight: content.implicitHeight

    CalendarWidget {
        id: content
        anchors.horizontalCenter: parent.horizontalCenter
    }
}
