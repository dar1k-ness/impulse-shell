import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
import Rustshell.Hyprland
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets

Scope {
    id: root
    
    LazyLoader {
        id: barLoader
        active: GlobalStates.barOpen
        component: Variants {
            model: Rustshell.screens
            delegate: PanelWindow { // Bar window
                id: barRoot
                required property var modelData
                screen: modelData
                exclusionMode: ExclusionMode.Ignore
                exclusiveZone: implicitHeight
                WlrLayershell.namespace: "rustshell:bar"

                anchors {
                    left: true
                    right: true
                    bottom: Config.options.waffles.bar.bottom
                    top: !Config.options.waffles.bar.bottom
                }

                color: "transparent"
                implicitHeight: content.implicitHeight
                implicitWidth: content.implicitWidth

                WaffleBarContent {
                    id: content
                    anchors.fill: parent
                }
            }
        }
    }

    IpcHandler {
        target: "bar"

        function toggle(): void {
            GlobalStates.barOpen = !GlobalStates.barOpen
        }

        function close(): void {
            GlobalStates.barOpen = false
        }

        function open(): void {
            GlobalStates.barOpen = true
        }
    }

    GlobalShortcut {
        name: "barToggle"
        description: "Toggles bar on press"

        onPressed: {
            GlobalStates.barOpen = !GlobalStates.barOpen;
        }
    }

    GlobalShortcut {
        name: "barOpen"
        description: "Opens bar on press"

        onPressed: {
            GlobalStates.barOpen = true;
        }
    }

    GlobalShortcut {
        name: "barClose"
        description: "Closes bar on press"

        onPressed: {
            GlobalStates.barOpen = false;
        }
    }
}
