import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets

QuickToggleModel {
    name: Translation.tr("Keep awake")

    toggled: Idle.inhibit
    icon: "coffee"
    mainAction: () => {
        Idle.toggleInhibit()
    }
    tooltipText: Translation.tr("Keep system awake")
}
