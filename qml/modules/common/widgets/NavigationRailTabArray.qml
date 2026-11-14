import ri.modules.common
import ri.modules.common.widgets
import QtQuick
import QtQuick.Layouts

Item {
    id: root
    property int currentIndex: 0
    property bool expanded: false
    // Offset of the buttons' indicators (NavigationRailButton.inset)
    property real inset: 0
    // The widest collapsed button (NavigationRailButton.collapsedWidth)
    readonly property real collapsedWidth: {
        let width = 0;
        for (let i = 0; i < tabBarColumn.children.length; i++)
            width = Math.max(width, tabBarColumn.children[i].collapsedWidth ?? 0);
        return width;
    }
    default property alias tabData: tabBarColumn.data  
    implicitHeight: tabBarColumn.implicitHeight
    implicitWidth: tabBarColumn.implicitWidth
    Layout.topMargin: 25

    Rectangle {
        property real itemHeight: tabBarColumn.children[0]?.baseSize ?? 56
        property real baseHighlightHeight: tabBarColumn.children[0]?.baseHighlightHeight ?? 56
        anchors {
            top: tabBarColumn.top
            left: tabBarColumn.left
            leftMargin: root.inset
            topMargin: itemHeight * root.currentIndex + (root.expanded ? 0 : ((itemHeight - baseHighlightHeight) / 2))
        }
        radius: Appearance.rounding.full
        color: Appearance.colors.colSecondaryContainer
        implicitHeight: root.expanded ? itemHeight : baseHighlightHeight
        implicitWidth: tabBarColumn?.children[root.currentIndex]?.visualWidth ?? 100

        Behavior on anchors.topMargin {
            NumberAnimation {
                duration: Appearance.animationCurves.expressiveFastSpatialDuration
                easing.type: Appearance.animation.elementMove.type
                easing.bezierCurve: Appearance.animationCurves.expressiveFastSpatial
            }
        }
    }

    ColumnLayout {
        id: tabBarColumn
        anchors.fill: parent
        spacing: 0
    }
}
