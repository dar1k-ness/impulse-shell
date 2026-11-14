import QtQuick
import QtQuick.Layouts
import Impulse

// One layout entry of a panel zone: the registry widget its `type` names, with its settings
// and the panel context (orientation, edge, zone hover, shortened form) its widget reads
// from its parent. A divided entry has a separator line before it, in its run's spacing,
// as between the widgets of one group on the original's vertical bar.
WidgetHost {
    id: host
    required property var modelData
    required property int index
    readonly property var entry: modelData
    property bool vertical: false
    property string edge: "top"
    property bool zoneHovered: false
    property int shortened: 0
    property bool divided: false
    property real dividerGap: 0
    property real dividerInset: 0
    property color dividerColor: "transparent"

    widgetType: entry.type
    settings: entry.settings
    // The widget's own `shown`, not its `visible`: that one reads back this host's visibility.
    visible: item !== null && (item.shown ?? true)
    implicitWidth: item ? item.implicitWidth : 0
    implicitHeight: item ? item.implicitHeight : 0

    Layout.alignment: vertical ? Qt.AlignHCenter : Qt.AlignVCenter
    // Widgets fill the panel's thickness unless they keep their own (the original's sidebar button).
    Layout.fillWidth: vertical ? (item?.fillThickness ?? true) : entry.width === "stretch"
    Layout.fillHeight: vertical ? entry.width === "stretch" : (item?.fillThickness ?? true)
    Layout.preferredWidth: !vertical && entry.width === "px" ? entry.px : -1
    Layout.preferredHeight: vertical && entry.width === "px" ? entry.px : -1
    // A widget may ask for room before it along the panel (the original's vertical sidebar button sits in a square).
    Layout.topMargin: divided ? dividerGap + 1 : (item?.leadingMargin ?? 0)

    Rectangle {
        visible: host.divided
        x: host.dividerInset
        y: -(host.dividerGap + 1)
        width: host.width - x * 2
        height: 1
        color: host.dividerColor
    }
}
