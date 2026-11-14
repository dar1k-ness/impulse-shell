pragma ComponentBehavior: Bound

import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.widgets.widgetCanvas
import QtQuick
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
import Rustshell.Hyprland

import ri.modules.rusty.background.widgets
import ri.modules.rusty.background.widgets.clock
import ri.modules.rusty.background.widgets.weather

Variants {
    id: root
    model: Rustshell.screens

    PanelWindow {
        id: panelRoot
        required property var modelData

        screen: modelData
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Bottom
        WlrLayershell.namespace: "rustshell:background"
        anchors {
            top: true
            bottom: true
            left: true
            right: true
        }
        color: "transparent"

        StyledImage {
            anchors.fill: parent
            source: Config.options.background.wallpaperPath
            fillMode: Image.PreserveAspectCrop
        }
    }
}
