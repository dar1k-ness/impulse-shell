import QtQuick
import QtQuick.Layouts
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.waffle.looks
import ri.modules.waffle.actionCenter
import Impulse

FooterRectangle {

    // Battery button
    WBorderlessButton {
        visible: Battery.available
        anchors.verticalCenter: parent.verticalCenter
        anchors.left: parent.left
        anchors.leftMargin: 12

        contentItem: Row {
            spacing: 4

            FluentIcon {
                anchors.verticalCenter: parent.verticalCenter
                icon: WIcons.batteryLevelIcon
                FluentIcon {
                    anchors.fill: parent
                    icon: WIcons.batteryIcon
                }
            }
            WText {
                anchors.verticalCenter: parent.verticalCenter
                text: `${Math.round(Battery.percentage * 100) || 0}%`
            }
        }
    }

    // Settings button
    WBorderlessButton {
        anchors.verticalCenter: parent.verticalCenter
        anchors.right: parent.right
        anchors.rightMargin: 12

        onClicked: {
            GlobalStates.sidebarLeftOpen = false;
            ShellControl.openWindow(Rustshell.shellPath("settings.qml"));
        }

        contentItem: FluentIcon {
            icon: "settings"
        }
    }
}
