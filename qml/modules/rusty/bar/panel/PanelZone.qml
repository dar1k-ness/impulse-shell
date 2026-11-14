import QtQuick
import QtQuick.Layouts

// One zone of a panel (start, center or end): its runs in layout order along the panel.
GridLayout {
    id: root
    property var runs: []
    property bool vertical: false
    property string edge: "top"
    property bool zoneHovered: false
    property int shortened: 0
    // Where free space goes when no run stretches: after the runs (start zone), before them
    // (end zone), or nowhere (center zone, sized to its runs).
    property bool fillBefore: false
    property bool fillAfter: false
    // A stretching run takes the free space only while it shows: the original hides the active window
    // title on a vertical or shortened bar, and the rest of the zone keeps its place. Only runs
    // are asked for `visible`: the fillers' visibility follows this property.
    readonly property bool stretches: {
        for (let i = 0; i < children.length; i++) {
            const run = children[i];
            if (run.modelData?.stretch && run.visible)
                return true;
        }
        return false;
    }
    property bool center: false
    property real spacing: 4

    columns: vertical ? 1 : -1
    columnSpacing: spacing
    rowSpacing: spacing

    Item {
        visible: root.fillBefore && !root.stretches
        Layout.fillWidth: !root.vertical
        Layout.fillHeight: root.vertical
    }
    Repeater {
        model: root.runs
        delegate: PanelRun {
            vertical: root.vertical
            edge: root.edge
            zoneHovered: root.zoneHovered
            shortened: root.shortened
            center: root.center
        }
    }
    Item {
        visible: root.fillAfter && !root.stretches
        Layout.fillWidth: !root.vertical
        Layout.fillHeight: root.vertical
    }
}
