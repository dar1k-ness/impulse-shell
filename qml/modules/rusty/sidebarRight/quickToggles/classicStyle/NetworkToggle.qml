import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import ri.modules.rusty.sidebarRight.quickToggles
import ri
import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Hyprland
import Impulse

QuickToggleButton {
    toggled: Network.wifiStatus !== "disabled"
    buttonIcon: Network.materialSymbol
    onClicked: Network.toggleWifi()
    altAction: () => {
        Rustshell.execDetached(["bash", "-c", `${Network.ethernet ? Config.options.apps.networkEthernet : Config.options.apps.network}`])
        GlobalStates.sidebarRightOpen = false
    }
    StyledToolTip {
        text: Translation.tr("%1 | Right-click to configure").arg(Network.networkName)
    }
}
