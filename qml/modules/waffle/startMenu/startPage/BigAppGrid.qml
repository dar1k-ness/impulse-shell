pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.modules.waffle.looks

GridLayout {
    id: root

    property list<var> desktopEntries: []

    columnSpacing: 0
    rowSpacing: 0

    uniformCellHeights: true
    uniformCellWidths: true

    Repeater {
        model: root.desktopEntries
        delegate: StartAppButton {
            id: pinnedAppButton
            required property var modelData
            desktopEntry: modelData
            onClicked: {
                GlobalStates.searchOpen = false;
                desktopEntry.execute();
            }
        }
    }
}
