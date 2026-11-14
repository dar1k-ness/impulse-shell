pragma ComponentBehavior: Bound
import ri
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.services
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
import Rustshell.Hyprland

// Options toolbar
Toolbar {
    id: root

    // Use a synchronizer on these
    property var action
    property var selectionMode
    // Signals
    signal dismiss()

    ToolbarTabBar {
        id: tabBar
        tabButtonList: [
            {"icon": "activity_zone", "name": Translation.tr("Rect")},
            {"icon": "gesture", "name": Translation.tr("Circle")}
        ]
        currentIndex: root.selectionMode === RegionSelection.SelectionMode.RectCorners ? 0 : 1
        onCurrentIndexChanged: {
            // Before the Synchronizer gives the mode, the binding above is still evaluating:
            // a write here is a binding loop on currentIndex (Qt 6.12 reports it too).
            if (root.selectionMode === undefined)
                return;
            root.selectionMode = currentIndex === 0 ? RegionSelection.SelectionMode.RectCorners : RegionSelection.SelectionMode.Circle;
        }
    }
}
