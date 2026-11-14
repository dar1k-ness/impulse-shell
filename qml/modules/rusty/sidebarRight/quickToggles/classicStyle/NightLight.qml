import QtQuick
import ri.modules.common
import ri.modules.common.widgets
import ri.services
import Rustshell.Io

QuickToggleButton {
    id: nightLightButton
    toggled: Hyprsunset.temperatureActive
    buttonIcon: Config.options.light.night.automatic ? "night_sight_auto" : "bedtime"
    onClicked: {
        Hyprsunset.toggleTemperature()
    }

    altAction: () => {
        Config.options.light.night.automatic = !Config.options.light.night.automatic
    }

    Component.onCompleted: {
        Hyprsunset.fetchState()
    }
    
    StyledToolTip {
        text: Translation.tr("Night Light | Right-click to toggle Auto mode")
    }
}
