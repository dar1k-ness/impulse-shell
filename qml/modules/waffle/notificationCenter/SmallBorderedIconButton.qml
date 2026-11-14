import QtQuick
import ri
import ri.services
import ri.modules.common
import ri.modules.waffle.looks

WBorderedButton {
    id: root
    implicitWidth: 24
    implicitHeight: 24
    contentItem: Item {
        FluentIcon {
            anchors.centerIn: parent
            implicitSize: 12
            icon: root.icon.name
            color: root.fgColor
        }
    }
}
