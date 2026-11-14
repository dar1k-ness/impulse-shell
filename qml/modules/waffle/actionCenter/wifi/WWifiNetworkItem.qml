import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Rustshell
import ri
import ri.services
import Impulse
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.modules.waffle.looks
import ri.modules.waffle.actionCenter

ExpandableChoiceButton {
    id: root
    required property WifiAccessPoint wifiNetwork

    contentItem: RowLayout {
        id: contentItem
        spacing: 12

        FluentIcon { // Duotone hack
            Layout.bottomMargin: 2
            Layout.alignment: Qt.AlignTop
            property int strength: root.wifiNetwork?.strength ?? 0
            icon: "wifi-1"
            implicitSize: 30
            color: Looks.colors.inactiveIcon

            FluentIcon { // Signal
                property int strength: root.wifiNetwork?.strength ?? 0
                icon: WIcons.wifiIconForStrength(strength)
                implicitSize: 30

                FluentIcon { // Security
                    anchors {
                        right: parent.right
                        bottom: parent.bottom
                    }
                    visible: root?.wifiNetwork?.isSecure ?? false
                    icon: "lock-closed"
                    filled: true
                    implicitSize: 14           
                }
            }
        }

        ColumnLayout {
            Layout.topMargin: statusText.visible ? 4 : 7
            Layout.bottomMargin: 4
            Layout.alignment: Qt.AlignTop
            Layout.fillWidth: true
            spacing: 1

            Behavior on Layout.topMargin {
                animation: Looks.transition.move.createObject(this)
            }

            WText { // Network name
                Layout.fillWidth: true
                elide: Text.ElideRight
                font.pixelSize: Looks.font.pixelSize.large
                text: root.wifiNetwork?.ssid ?? Translation.tr("Unknown")
                textFormat: Text.PlainText
            }
            WText { // Status
                id: statusText
                Layout.fillWidth: true
                elide: Text.ElideRight
                text: root.wifiNetwork?.active ? Translation.tr("Connected") : root.wifiNetwork?.isSecure ? Translation.tr("Secured") : Translation.tr("Not secured")
                font.pixelSize: Looks.font.pixelSize.large
                color: Looks.colors.subfg
                visible: root.wifiNetwork?.active || root.expanded || passwordPrompt.visible
                Behavior on opacity {
                    animation: Looks.transition.opacity.createObject(this)
                }
            }

            ColumnLayout { // Password, asked for by Network.connectToWifiNetwork
                id: passwordPrompt
                Layout.fillWidth: true
                Layout.topMargin: 6
                spacing: 8
                visible: root.wifiNetwork?.askingPassword ?? false
                onVisibleChanged: {
                    if (visible)
                        passwordField.forceActiveFocus();
                    else
                        passwordField.clear();
                }

                WTextField {
                    id: passwordField
                    Layout.fillWidth: true
                    placeholderText: Translation.tr("Password")
                    echoMode: TextInput.Password
                    inputMethodHints: Qt.ImhSensitiveData
                    onAccepted: Network.changePassword(root.wifiNetwork, passwordField.text)
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 4

                    WButton {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        checked: true
                        colBackground: Looks.colors.bg2
                        colBackgroundHover: Looks.colors.bg2Hover
                        colBackgroundActive: Looks.colors.bg2Active
                        implicitHeight: 30
                        text: Translation.tr("Connect")
                        onClicked: Network.changePassword(root.wifiNetwork, passwordField.text)
                    }
                    WButton {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        colBackground: Looks.colors.bg2
                        colBackgroundHover: Looks.colors.bg2Hover
                        colBackgroundActive: Looks.colors.bg2Active
                        implicitHeight: 30
                        text: Translation.tr("Cancel")
                        onClicked: root.wifiNetwork.askingPassword = false
                    }
                }
            }

            WButton {
                Layout.alignment: Qt.AlignRight
                horizontalAlignment: Text.AlignHCenter
                visible: root.expanded && !passwordPrompt.visible
                checked: !(root.wifiNetwork?.active ?? false)
                colBackground: Looks.colors.bg2
                colBackgroundHover: Looks.colors.bg2Hover
                colBackgroundActive: Looks.colors.bg2Active
                implicitHeight: 30
                implicitWidth: 148
                text: root.wifiNetwork?.active ? Translation.tr("Disconnect") : Translation.tr("Connect")

                onClicked: {
                    if (root.wifiNetwork?.active) {
                        Network.disconnectWifiNetwork();
                    } else {
                        Network.connectToWifiNetwork(root.wifiNetwork);
                    }
                }
            }
        }
    }
}
