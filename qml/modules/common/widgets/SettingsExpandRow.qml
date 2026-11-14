import QtQuick
import ri.modules.common
import ri.modules.common.widgets

/**
 * A row that reveals the rows bound to `expanded` below it (`shown: thisRow.expanded`).
 */
SettingsRow {
    id: root
    property bool expanded: false
    clickable: true
    showChildToggle: false
    onClicked: root.expanded = !root.expanded

    MaterialSymbol {
        text: "chevron_right"
        iconSize: 22
        rotation: root.expanded ? 90 : 0
        color: Appearance.colors.colOnLayer2
        Behavior on rotation {
            animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
        }
    }
}
