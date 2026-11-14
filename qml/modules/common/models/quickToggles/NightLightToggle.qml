import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets

QuickToggleModel {
    property bool auto: Config.options.light.night.automatic

    name: Translation.tr("Night Light")
    statusText: (auto ? Translation.tr("Auto, ") : "") + (toggled ? Translation.tr("Active") : Translation.tr("Inactive"))

    toggled: Hyprsunset.temperatureActive
    icon: auto ? "night_sight_auto" : "bedtime"
    
    mainAction: () => {
        Hyprsunset.toggleTemperature()
    }
    hasMenu: true

    Component.onCompleted: {
        Hyprsunset.fetchState()
    }
    
    tooltipText: Translation.tr("Night Light | Right-click to configure")
}
