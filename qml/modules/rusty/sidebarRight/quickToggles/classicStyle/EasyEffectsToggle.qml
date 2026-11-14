import ri.modules.common.widgets
import ri
import ri.services
import Impulse
import QtQuick
import Rustshell.Io
import Rustshell
import Rustshell.Hyprland

QuickToggleButton {
    id: root
    visible: EasyEffects.available
    toggled: EasyEffects.active
    buttonIcon: "instant_mix"

    Component.onCompleted: {
        EasyEffects.fetchActiveState()
    }

    onClicked: {
        EasyEffects.toggle()
    }

    altAction: () => {
        EasyEffects.open()
        GlobalStates.sidebarRightOpen = false
    }

    StyledToolTip {
        text: Translation.tr("EasyEffects | Right-click to configure")
    }
}
