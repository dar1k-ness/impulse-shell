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
    // The zone has room for the widget: one it has none for waits, hidden (PanelZone.fit).
    property bool kept: true
    // Laid out off screen for its length only (PanelContent's probes): not one edit mode frames.
    property bool probe: false

    surface: "panel"
    widgetType: entry.type
    // The edit overlay finds a widget by it on this monitor's layout; one of an unplugged
    // monitor shown here until it comes back is edited there.
    layoutId: !probe && (entry.guest ?? "") === "" ? entry.id : ""
    settings: entry.settings
    // The widget's own `shown`, not its `visible`: that one reads back this host's visibility.
    visible: item !== null && (item.shown ?? true) && kept
    implicitWidth: item ? item.implicitWidth : 0
    implicitHeight: item ? item.implicitHeight : 0

    Layout.alignment: vertical ? Qt.AlignHCenter : Qt.AlignVCenter
    // Widgets fill the panel's thickness unless they keep their own (the original's sidebar button).
    Layout.fillWidth: vertical ? (item?.fillThickness ?? true) : entry.width === "stretch"
    Layout.fillHeight: vertical ? entry.width === "stretch" : (item?.fillThickness ?? true)
    Layout.preferredWidth: !vertical && entry.width === "px" ? entry.px : -1
    Layout.preferredHeight: vertical && entry.width === "px" ? entry.px : -1
    // Along the panel a widget never gets less than its natural length (or the length it was
    // given); a stretching one gives way down to the registry's least size for it (the
    // spacer's is 0), never asking for more than its natural length, and paints no further
    // than the length it got.
    readonly property real naturalLength: vertical ? implicitHeight : implicitWidth
    readonly property real minimumLength: entry.width === "stretch" ? Math.min(vertical ? entry.minHeight : entry.minWidth, naturalLength) : entry.width === "px" ? entry.px : naturalLength
    clip: entry.width === "stretch" && (vertical ? height < implicitHeight : width < implicitWidth)
    Layout.minimumWidth: vertical ? -1 : minimumLength
    Layout.minimumHeight: vertical ? minimumLength : -1
    // A widget may ask for room before it along the panel (the original's vertical sidebar button sits in a square).
    readonly property real leadingSpace: divided ? dividerGap + 1 : (item?.leadingMargin ?? 0)
    Layout.topMargin: leadingSpace

    Rectangle {
        visible: host.divided
        x: host.dividerInset
        y: -(host.dividerGap + 1)
        width: host.width - x * 2
        height: 1
        color: host.dividerColor
    }
}
