import QtQuick
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import Impulse

QuickToggleModel {
    name: Translation.tr("Internet")
    statusText: Network.networkName
    tooltipText: Translation.tr("%1 | Right-click to configure").arg(Network.networkName)
    icon: Network.materialSymbol

    toggled: Network.wifiStatus !== "disabled"
    mainAction: () => Network.toggleWifi()
    hasMenu: true
}
