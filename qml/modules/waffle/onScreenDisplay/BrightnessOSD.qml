import QtQuick
import Rustshell
import Rustshell.Hyprland
import ri.services
import ri.modules.waffle.looks

OSDValue {
    id: root
    property var focusedScreen: Rustshell.screens.find(s => s.name === Hyprland.focusedMonitor?.name)
    property var brightnessMonitor: Brightness.getMonitorForScreen(focusedScreen)
    iconName: "weather-sunny"
    value: brightnessMonitor?.brightness ?? 0
    showNumber: false

    Connections {
        target: Brightness
        function onBrightnessChanged() {
            root.timer.restart();
        }
    }
}
