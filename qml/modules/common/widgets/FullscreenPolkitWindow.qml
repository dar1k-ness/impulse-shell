pragma ComponentBehavior: Bound
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import QtQuick
import Rustshell
import Rustshell.Wayland

Scope {
    id: root
    required property Component contentComponent
    
    Loader {
        active: PolkitService.active
        sourceComponent: Variants {
            model: Rustshell.screens
            delegate: PanelWindow {
                id: panelWindow
                required property var modelData
                screen: modelData
                
                anchors {
                    top: true
                    left: true
                    right: true
                    bottom: true
                }

                color: "transparent"
                WlrLayershell.namespace: "rustshell:polkit"
                WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
                WlrLayershell.layer: WlrLayer.Overlay
                exclusionMode: ExclusionMode.Ignore

                Loader {
                    anchors.fill: parent
                    sourceComponent: root.contentComponent
                }
            }
        }
    }
}
