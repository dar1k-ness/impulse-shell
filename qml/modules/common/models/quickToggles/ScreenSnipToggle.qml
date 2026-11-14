import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import Impulse

QuickToggleModel {
    name: Translation.tr("Screen snip")
    hasStatusText: false
    toggled: false
    icon: "screenshot_region"

    // The region selector shows the screen as it is when it opens. ShellControl.ipcCall runs
    // the handler once the compositor has the closed sidebar off the screen.
    mainAction: () => {
        GlobalStates.sidebarRightOpen = false;
        ShellControl.ipcCall("region", "screenshot");
    }

    tooltipText: Translation.tr("Screen snip")
}
