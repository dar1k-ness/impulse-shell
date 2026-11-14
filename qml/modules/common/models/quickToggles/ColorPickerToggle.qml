import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import Impulse

QuickToggleModel {
    id: root
    name: Translation.tr("Color picker")
    hasStatusText: false
    toggled: false
    icon: "colorize"

    // The panel this toggle is in, when closing the right sidebar closes it (set by that panel's
    // button). hyprpicker freezes the screen as it starts, so it starts once the panel is off
    // screen; without a panel it starts at once.
    property var panel: null

    mainAction: () => {
        ColorPicker.pickOnceHidden(root.panel);
        GlobalStates.sidebarRightOpen = false;
    }

    tooltipText: Translation.tr("Color picker")
}
