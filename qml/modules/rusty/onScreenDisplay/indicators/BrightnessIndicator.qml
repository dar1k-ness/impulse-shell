import ri.services
import QtQuick
import Rustshell
import Rustshell.Hyprland
import ri.modules.rusty.onScreenDisplay

OsdValueIndicator {
    id: root
    property var focusedScreen: Rustshell.screens.find(s => s.name === Hyprland.focusedMonitor?.name)
    property var brightnessMonitor: Brightness.getMonitorForScreen(focusedScreen)

    icon: Hyprsunset.temperatureActive ? "routine" : "light_mode"
    rotateIcon: true
    scaleIcon: true
    name: Translation.tr("Brightness")
    value: root.brightnessMonitor?.brightness ?? 50
}
