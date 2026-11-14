import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets

QuickToggleModel {
    name: Translation.tr("Dark Mode")
    statusText: Appearance.m3colors.darkmode ? Translation.tr("Dark") : Translation.tr("Light")

    toggled: Appearance.m3colors.darkmode
    icon: "contrast"
    
    mainAction: () => {
        Switchwall.run({ mode: Appearance.m3colors.darkmode ? "light" : "dark", noSwitch: true });
    }

    tooltipText: Translation.tr("Dark Mode")
}
