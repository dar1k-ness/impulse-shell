pragma ComponentBehavior: Bound
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import Qt.labs.synchronizer
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
import Rustshell.Hyprland

Scope {
    id: overviewScope

    Variants {
        id: overviewVariants
        model: Rustshell.screens

        Loader {
            id: panelLoader
            required property var modelData
            active: false
            Connections {
                target: GlobalStates
                function onOverviewOpenChanged() {
                    if (GlobalStates.overviewOpen)
                        panelLoader.active = true;
                }
            }
            sourceComponent: PanelWindow {
                id: root
                property string searchingText: ""
                readonly property HyprlandMonitor monitor: Hyprland.monitorFor(root.screen)
                property bool monitorIsFocused: (Hyprland.focusedMonitor?.id == monitor?.id)
                screen: panelLoader.modelData

                WlrLayershell.namespace: "rustshell:wTaskView"
                WlrLayershell.layer: WlrLayer.Overlay
                WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
                color: "transparent"

                anchors {
                    top: true
                    bottom: true
                    left: true
                    right: true
                }

                TaskViewContent {
                    id: taskViewContent
                    anchors.fill: parent

                    Component.onCompleted: {
                        taskViewContent.forceActiveFocus();
                    }
                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Escape) {
                            GlobalStates.overviewOpen = false;
                        }
                    }

                    Connections {
                        target: GlobalStates
                        function onOverviewOpenChanged() {
                            if (!GlobalStates.overviewOpen)
                                taskViewContent.close();
                        }
                    }
                    onClosed: panelLoader.active = false
                }
            }
        }
    }

    IpcHandler {
        target: "taskView"

        function toggle() {
            GlobalStates.overviewOpen = !GlobalStates.overviewOpen;
        }
        function close() {
            GlobalStates.overviewOpen = false;
        }
        function open() {
            GlobalStates.overviewOpen = true;
        }
    }

    GlobalShortcut {
        name: "overviewWorkspacesToggle"
        description: "Toggles overview on press"

        onPressed: {
            GlobalStates.overviewOpen = !GlobalStates.overviewOpen;
        }
    }
}
