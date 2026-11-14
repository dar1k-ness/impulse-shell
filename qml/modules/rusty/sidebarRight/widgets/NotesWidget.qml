import QtQuick
import ri.modules.common
import ri.modules.rusty.sidebarRight.todo

// Sidebar widget "notes": the to-do list on its own card.
Rectangle {
    id: root
    radius: Appearance.rounding.normal
    color: Appearance.colors.colLayer1
    property int cardHeight: 350
    implicitHeight: root.cardHeight

    TodoWidget {
        anchors.fill: parent
        anchors.margins: 10
    }
}
