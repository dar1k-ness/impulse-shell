import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
import Rustshell.Hyprland

Scope {
    id: root

    Loader {
        id: sessionLoader
        active: GlobalStates.sessionOpen

        Connections {
            target: GlobalStates
            function onScreenLockedChanged() {
                if (GlobalStates.screenLocked) {
                    GlobalStates.sessionOpen = false;
                }
            }
        }

        sourceComponent: PanelWindow { // Session menu
            id: sessionRoot
            visible: sessionLoader.active

            function hide() {
                GlobalStates.sessionOpen = false;
            }

            exclusionMode: ExclusionMode.Ignore
            WlrLayershell.namespace: "rustshell:session"
            WlrLayershell.layer: WlrLayer.Overlay
            WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
            // This is a big surface so we needa carefully choose the transparency,
            // or we'll get a large scary rgb blob
            color: "#000000"

            anchors {
                top: true
                left: true
                right: true
                bottom: true
            }

            Item {
                anchors.fill: parent
                Keys.onPressed: (event) => {
                    if (event.key === Qt.Key_Escape) {
                        sessionRoot.hide();
                    }
                }

                SessionScreenContent {
                    anchors.fill: parent
                }
            }
        }
    }

    IpcHandler {
        target: "session"

        function toggle(): void {
            GlobalStates.sessionOpen = !GlobalStates.sessionOpen;
        }

        function close(): void {
            GlobalStates.sessionOpen = false
        }

        function open(): void {
            GlobalStates.sessionOpen = true
        }
    }

    GlobalShortcut {
        name: "sessionToggle"
        description: "Toggles session screen on press"

        onPressed: {
            GlobalStates.sessionOpen = !GlobalStates.sessionOpen;
        }
    }

    GlobalShortcut {
        name: "sessionOpen"
        description: "Opens session screen on press"

        onPressed: {
            GlobalStates.sessionOpen = true
        }
    }

    GlobalShortcut {
        name: "sessionClose"
        description: "Closes session screen on press"

        onPressed: {
            GlobalStates.sessionOpen = false
        }
    }

}
