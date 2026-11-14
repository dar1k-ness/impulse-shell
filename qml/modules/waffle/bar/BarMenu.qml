import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Rustshell
import Rustshell.Hyprland
import ri.modules.common
import ri.modules.common.functions
import ri.modules.waffle.looks

BarPopup {
    id: root
    default property var menuData
    property var model: []
    padding: 2

    contentItem: ColumnLayout {
        anchors.centerIn: parent
        spacing: 0

        Repeater {
            model: root.model
            delegate: DelegateChooser {
                role: "type"
                DelegateChoice {
                    roleValue: "separator"
                    Rectangle {
                        Layout.topMargin: 2
                        Layout.bottomMargin: 2
                        Layout.fillWidth: true
                        implicitHeight: 1
                        color: Looks.colors.bg0Border
                    }
                }
                DelegateChoice {
                    roleValue: undefined
                    WButton {
                        id: btn
                        Layout.fillWidth: true
                        inset: 2

                        required property var modelData
                        icon.name: modelData.iconName ? modelData.iconName : ""
                        monochromeIcon: modelData.monochromeIcon ?? true
                        text: modelData.text ? modelData.text : ""

                        onClicked: {
                            if (modelData.action) modelData.action();
                            root.close();
                        }
                    }
                }
            }
        }
    }
}
