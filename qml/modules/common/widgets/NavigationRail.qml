import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

ColumnLayout { // Window content with navigation rail and content pane
    id: root
    property bool expanded: true
    property int currentIndex: 0
    property real baseSize: 56
    // Width of the collapsed column, at least baseSize (its labels may be wider)
    property real collapsedWidth: baseSize
    // Collapsed, the items are centred in the column: their offset from its left edge
    readonly property real restingInset: Math.max(0, (collapsedWidth - baseSize) / 2)
    property real inset: expanded ? 0 : restingInset
    Behavior on inset {
        animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
    }
    spacing: 5
}
