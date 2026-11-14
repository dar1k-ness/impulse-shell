import QtQuick
import ri.modules.common
import ri.modules.common.functions
import ri.modules.rusty.bar as Bar

Bar.LeftSidebarButton {
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    // the original's bar does not stretch this button across itself: it stays a circle in the middle.
    readonly property bool fillThickness: false
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    readonly property real leadingMargin: vertical ? (Appearance.sizes.baseVerticalBarWidth - implicitWidth) / 2 : 0
    colBackground: zoneHovered ? Appearance.colors.colLayer1Hover : ColorUtils.transparentize(Appearance.colors.colLayer1Hover, 1)
}
