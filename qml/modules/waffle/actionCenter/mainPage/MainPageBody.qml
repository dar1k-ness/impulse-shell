import QtQuick
import QtQuick.Layouts
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.waffle.looks
import ri.modules.waffle.actionCenter

BodyRectangle {
    id: root
    implicitHeight: contentLayout.implicitHeight

    ColumnLayout {
        id: contentLayout
        anchors.fill: parent
        spacing: 0

        MainPageBodyToggles {
            id: togglesContainer
            Layout.fillWidth: true
        }

        Rectangle {
            implicitHeight: 1
            Layout.fillWidth: true
            color: Looks.colors.bg1Border
        }

        MainPageBodySliders {
            Layout.margins: 12
            Layout.topMargin: 18
            Layout.bottomMargin: 14
        }
    }
}
