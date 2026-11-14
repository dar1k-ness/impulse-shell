import QtQuick
import QtQuick.Layouts
import Rustshell
import org.kde.kirigami as Kirigami
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import ri.modules.waffle.looks

Item {
    id: root

    property string icon: ""
    property real implicitSize: 16
    implicitWidth: implicitSize
    implicitHeight: implicitSize

    Kirigami.Icon {
        anchors.fill: parent
        implicitWidth: root.implicitSize
        implicitHeight: root.implicitSize

        source: root.icon || fallback
        fallback: `${Looks.iconsPath}/apps.svg`
        roundToIconSize: false
        isMask: !root.icon
        color: Looks.colors.fg
    }
}
