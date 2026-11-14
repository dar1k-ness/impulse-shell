import QtQuick
import ri.modules.common

/**
 * The card of a sidebar widget that is a block of its own or, narrower than the sidebar, a cell
 * in a row of toggles. There it is `cellSize` grid cells wide (`cellsWidth`) and drawn as the
 * toggles are; the widget shows what fits a cell that wide.
 */
Rectangle {
    id: root
    // From the sidebar's grid (WidgetHost.placement); `cellSize` 0: a block of its own.
    property int cellSize: 0
    property real baseCellWidth: 0
    property real baseCellHeight: 56
    property real cellSpacing: 0
    readonly property bool inGrid: root.cellSize > 0
    readonly property real cellsWidth: root.baseCellWidth * root.cellSize + root.cellSpacing * (root.cellSize - 1)
    radius: root.inGrid ? Math.min(Appearance.rounding.large, root.height / 2) : Appearance.rounding.normal
    color: root.inGrid ? Appearance.colors.colLayer2 : Appearance.colors.colLayer1
}
