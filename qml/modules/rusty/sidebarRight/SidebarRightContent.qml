import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Rustshell
import Rustshell.Bluetooth
import Rustshell.Hyprland
import Impulse

import ri.modules.rusty.sidebarRight.quickToggles
import ri.modules.rusty.sidebarRight.quickToggles.classicStyle

import ri.modules.rusty.sidebarRight.bluetoothDevices
import ri.modules.rusty.sidebarRight.nightLight
import ri.modules.rusty.sidebarRight.volumeMixer
import ri.modules.rusty.sidebarRight.wifiNetworks
import Impulse

Item {
    id: root
    property int sidebarWidth: Appearance.sizes.sidebarWidth
    property int sidebarPadding: 10
    property string settingsQmlPath: Rustshell.shellPath("settings.qml")
    property bool showAudioOutputDialog: false
    property bool showAudioInputDialog: false
    property bool showBluetoothDialog: false
    property bool showNightLightDialog: false
    property bool showWifiDialog: false
    property bool editMode: false
    // The layout's sidebar container places the widgets (rusty-impulse); otherwise the original's fixed blocks.
    readonly property bool layoutPlaced: LayoutState.hasContainer("sidebar")

    Connections {
        target: GlobalStates
        // The sidebar opens as it first did (docs/original-differences.md): the state is put back as
        // it closes, so nothing is seen moving, and as it opens.
        function onSidebarRightOpenChanged() {
            root.showWifiDialog = false;
            root.showBluetoothDialog = false;
            root.showNightLightDialog = false;
            root.showAudioOutputDialog = false;
            root.showAudioInputDialog = false;
            root.editMode = false;
        }
    }

    implicitHeight: sidebarRightBackground.implicitHeight
    implicitWidth: sidebarRightBackground.implicitWidth

    StyledRectangularShadow {
        target: sidebarRightBackground
    }
    Rectangle {
        id: sidebarRightBackground

        anchors.fill: parent
        implicitHeight: parent.height - Appearance.sizes.hyprlandGapsOut * 2
        implicitWidth: sidebarWidth - Appearance.sizes.hyprlandGapsOut * 2
        color: Appearance.colors.colLayer0
        border.width: 1
        border.color: Appearance.colors.colLayer0Border
        radius: Appearance.rounding.screenRounding - Appearance.sizes.hyprlandGapsOut + 1

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: sidebarPadding
            spacing: sidebarPadding

            SystemButtonRow {
                Layout.fillHeight: false
                Layout.fillWidth: true
                // Layout.margins: 10
                Layout.topMargin: 5
                Layout.bottomMargin: 0
            }

            Loader {
                id: slidersLoader
                Layout.fillWidth: true
                visible: active
                active: {
                    const configQuickSliders = Config.options.sidebar.quickSliders
                    if (root.layoutPlaced || !configQuickSliders.enable) return false
                    if (!configQuickSliders.showMic && !configQuickSliders.showVolume && !configQuickSliders.showBrightness) return false;
                    return true;
                }
                sourceComponent: QuickSliders {}
            }

            LoaderedQuickPanelImplementation {
                styleName: "classic"
                sourceComponent: ClassicQuickPanel {}
            }

            LoaderedQuickPanelImplementation {
                styleName: "android"
                sourceComponent: AndroidQuickPanel {
                    editMode: root.editMode
                }
            }

            // the original's own groups are not created while the layout's sidebar replaces them.
            Loader {
                visible: active
                active: !root.layoutPlaced
                Layout.alignment: Qt.AlignHCenter
                Layout.fillHeight: true
                Layout.fillWidth: true
                sourceComponent: CenterWidgetGroup {}
            }

            Loader {
                visible: active
                active: !root.layoutPlaced
                Layout.alignment: Qt.AlignHCenter
                Layout.fillHeight: false
                Layout.fillWidth: true
                Layout.preferredHeight: implicitHeight
                sourceComponent: BottomWidgetGroup {}
            }

            Loader {
                id: layoutLoader
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: active
                active: root.layoutPlaced
                source: "SidebarLayoutContent.qml"
                onLoaded: {
                    layoutLoader.item.connector = root.QsWindow.window?.screen?.name ?? "";
                    layoutLoader.item.editMode = root.editMode;
                }
                Connections {
                    target: root
                    function onEditModeChanged() {
                        if (layoutLoader.item)
                            layoutLoader.item.editMode = root.editMode;
                    }
                }
                Connections {
                    target: layoutLoader.item
                    ignoreUnknownSignals: true
                    function onOpenAudioOutputDialog() {
                        root.showAudioOutputDialog = true;
                    }
                    function onOpenAudioInputDialog() {
                        root.showAudioInputDialog = true;
                    }
                    function onOpenBluetoothDialog() {
                        root.showBluetoothDialog = true;
                    }
                    function onOpenNightLightDialog() {
                        root.showNightLightDialog = true;
                    }
                    function onOpenWifiDialog() {
                        root.showWifiDialog = true;
                    }
                }
            }
        }
    }

    ToggleDialog {
        shownPropertyString: "showAudioOutputDialog"
        dialog: VolumeDialog {
            isSink: true
        }
    }

    ToggleDialog {
        shownPropertyString: "showAudioInputDialog"
        dialog: VolumeDialog {
            isSink: false
        }
    }

    ToggleDialog {
        shownPropertyString: "showBluetoothDialog"
        dialog: BluetoothDialog {}
        onShownChanged: {
            if (!shown) {
                Bluetooth.defaultAdapter.discovering = false;
            } else {
                Bluetooth.defaultAdapter.enabled = true;
                Bluetooth.defaultAdapter.discovering = true;
            }
        }
    }

    ToggleDialog {
        shownPropertyString: "showNightLightDialog"
        dialog: NightLightDialog {}
    }

    ToggleDialog {
        shownPropertyString: "showWifiDialog"
        dialog: WifiDialog {}
        onShownChanged: {
            if (!shown) return;
            Network.enableWifi();
            Network.rescanWifi();
        }
    }

    component ToggleDialog: Loader {
        id: toggleDialogLoader
        required property string shownPropertyString
        property alias dialog: toggleDialogLoader.sourceComponent
        readonly property bool shown: root[shownPropertyString]
        anchors.fill: parent

        // Cleared by the sidebar's reset and not by the dialog: the dialog closes as on dismiss.
        onShownChanged: {
            if (shown) toggleDialogLoader.active = true;
            else if (toggleDialogLoader.item) toggleDialogLoader.item.show = false;
        }
        active: shown
        onActiveChanged: {
            if (active) {
                item.show = true;
                item.forceActiveFocus();
            }
        }
        Connections {
            target: toggleDialogLoader.item
            function onDismiss() {
                toggleDialogLoader.item.show = false
                root[toggleDialogLoader.shownPropertyString] = false;
            }
            function onVisibleChanged() {
                if (!toggleDialogLoader.item.visible && !root[toggleDialogLoader.shownPropertyString]) toggleDialogLoader.active = false;
            }
        }
    }

    component LoaderedQuickPanelImplementation: Loader {
        id: quickPanelImplLoader
        required property string styleName
        Layout.alignment: item?.Layout.alignment ?? Qt.AlignHCenter
        Layout.fillWidth: item?.Layout.fillWidth ?? false
        visible: active
        active: !root.layoutPlaced && Config.options.sidebar.quickToggles.style === styleName
        Connections {
            target: quickPanelImplLoader.item
            function onOpenAudioOutputDialog() {
                root.showAudioOutputDialog = true;
            }
            function onOpenAudioInputDialog() {
                root.showAudioInputDialog = true;
            }
            function onOpenBluetoothDialog() {
                root.showBluetoothDialog = true;
            }
            function onOpenNightLightDialog() {
                root.showNightLightDialog = true;
            }
            function onOpenWifiDialog() {
                root.showWifiDialog = true;
            }
        }
    }

    component SystemButtonRow: Item {
        implicitHeight: Math.max(uptimeContainer.implicitHeight, systemButtonsRow.implicitHeight)

        Rectangle {
            id: uptimeContainer
            anchors {
                top: parent.top
                bottom: parent.bottom
                left: parent.left
            }
            color: Appearance.colors.colLayer1
            radius: height / 2
            implicitWidth: uptimeRow.implicitWidth + 24
            implicitHeight: uptimeRow.implicitHeight + 8
            
            Row {
                id: uptimeRow
                anchors.centerIn: parent
                spacing: 8
                CustomIcon {
                    id: distroIcon
                    anchors.verticalCenter: parent.verticalCenter
                    width: 25
                    height: 25
                    source: SystemInfo.distroIcon
                    colorize: true
                    color: Appearance.colors.colOnLayer0
                }
                StyledText {
                    anchors.verticalCenter: parent.verticalCenter
                    font.pixelSize: Appearance.font.pixelSize.normal
                    color: Appearance.colors.colOnLayer0
                    text: Translation.tr("Up %1").arg(DateTime.uptime)
                    textFormat: Text.MarkdownText
                }
            }
        }

        ButtonGroup {
            id: systemButtonsRow
            anchors {
                top: parent.top
                bottom: parent.bottom
                right: parent.right
            }
            color: Appearance.colors.colLayer1
            padding: 4

            QuickToggleButton {
                toggled: root.editMode
                visible: Config.options.sidebar.quickToggles.style === "android"
                buttonIcon: "edit"
                onClicked: root.editMode = !root.editMode
                StyledToolTip {
                    text: Translation.tr("Edit quick toggles") + (root.editMode ? Translation.tr("\nLMB to enable/disable\nRMB to toggle size\nScroll to swap position") : "")
                }
            }
            QuickToggleButton {
                toggled: false
                buttonIcon: "restart_alt"
                onClicked: hyprlandReload.send()
                // The shell reloads once Hyprland has answered (or could not be reached):
                // reloading at once would end the shell before the request is sent.
                HyprlandRequest {
                    id: hyprlandReload
                    command: "reload"
                    onReplied: Rustshell.reload(true)
                }
                StyledToolTip {
                    text: Translation.tr("Reload Hyprland & shell")
                }
            }
            QuickToggleButton {
                toggled: false
                buttonIcon: "settings"
                onClicked: {
                    GlobalStates.sidebarRightOpen = false;
                    ShellControl.openWindow(root.settingsQmlPath);
                }
                StyledToolTip {
                    text: Translation.tr("Settings")
                }
            }
            QuickToggleButton {
                toggled: false
                buttonIcon: "power_settings_new"
                onClicked: {
                    GlobalStates.sessionOpen = true;
                }
                StyledToolTip {
                    text: Translation.tr("Session")
                }
            }
        }
    }
}
