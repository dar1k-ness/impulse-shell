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

    // Edit mode carries the widget along its panel with the pointer: how far from its place,
    // written by the edit controller while it is carried (`carried`). It goes above its
    // neighbours on a chip of its own, so what the pointer holds is the widget itself; let go
    // somewhere it cannot land, it slides back.
    property color chipColor: "transparent"
    property color chipBorderColor: "transparent"
    property real chipRadius: 0
    property bool carried: false
    property real carryX: 0
    property real carryY: 0
    property real shiftX: carryX
    property real shiftY: carryY
    Behavior on shiftX {
        enabled: !host.carried
        NumberAnimation {
            duration: 150
            easing.type: Easing.OutCubic
        }
    }
    Behavior on shiftY {
        enabled: !host.carried
        NumberAnimation {
            duration: 150
            easing.type: Easing.OutCubic
        }
    }
    // The length the layout gives the widget and the room it keeps beside it along the panel
    // (a gap of the layout: what an end pulled in in edit mode leaves, so the other end and
    // the widgets around stay where they were). The entry has them as the widget was made;
    // a later change of these alone, or what an edit mode pull would give while it lasts, is
    // written here by the panel (`set`), so the zone keeps its widgets and only lays them out.
    property bool set: false
    property string setWidth: "auto"
    property real setPx: 0
    property real setLead: 0
    property real setTrail: 0
    readonly property string lengthKind: set ? setWidth : entry.width
    readonly property real lengthPx: set ? setPx : entry.px
    readonly property real leadGap: set ? setLead : (entry.lead ?? 0)
    readonly property real trailGap: set ? setTrail : (entry.trail ?? 0)
    readonly property bool lifted: carried || shiftX !== 0 || shiftY !== 0
    z: lifted ? 1 : 0
    transform: Translate {
        x: host.shiftX
        y: host.shiftY
    }

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
    Layout.fillWidth: vertical ? (item?.fillThickness ?? true) : lengthKind === "stretch"
    Layout.fillHeight: vertical ? lengthKind === "stretch" : (item?.fillThickness ?? true)
    Layout.preferredWidth: !vertical && lengthKind === "px" ? lengthPx : -1
    Layout.preferredHeight: vertical && lengthKind === "px" ? lengthPx : -1
    // The length the layout fixes it at, -1 when it takes its own or stretches: a widget shows
    // what fits that length, as the player does with its title.
    readonly property real fixedLength: lengthKind === "px" ? lengthPx : -1
    // Along the panel a widget never gets less than its natural length (or the length it was
    // given); a stretching one gives way down to the least length its widget names (the player
    // cuts its title) or the registry has for a type of many sizes (the spacer's is 0), never
    // asking for more than its natural length: a clock that stretches is never cut to a strip.
    // Neither one paints further than the length it got, so nothing of it lies over its
    // neighbours.
    readonly property real naturalLength: vertical ? implicitHeight : implicitWidth
    readonly property real leastLength: item?.leastLength ?? (vertical ? entry.minHeight : entry.minWidth)
    readonly property real minimumLength: lengthKind === "stretch" ? Math.min(leastLength, naturalLength) : lengthKind === "px" ? lengthPx : naturalLength
    // The least length edit mode lets it be pulled to: what its widget names for that (the
    // player keeps its title) or for stretching (the window title is cut with an ellipsis),
    // else its natural length, at which none of what it shows is cut.
    readonly property real contentLength: item?.contentLength ?? item?.leastLength ?? naturalLength
    // The length edit mode draws its outline around while it is shorter: what a widget that
    // shows the same at any length names (the clock's text with room to its handles).
    readonly property real clearLength: item?.clearLength ?? 0
    clip: lengthKind !== "auto" && (vertical ? height < implicitHeight : width < implicitWidth)
    Layout.minimumWidth: vertical ? -1 : minimumLength
    Layout.minimumHeight: vertical ? minimumLength : -1
    // A widget may ask for room before it along the panel (the original's vertical sidebar button sits in a square).
    readonly property real leadingSpace: divided ? dividerGap + 1 : (item?.leadingMargin ?? 0)
    Layout.topMargin: leadingSpace + (vertical ? leadGap : 0)
    Layout.bottomMargin: vertical ? trailGap : 0
    Layout.leftMargin: vertical ? 0 : leadGap
    Layout.rightMargin: vertical ? 0 : trailGap
    // What it takes along the panel besides its own length.
    readonly property real besideLength: leadGap + trailGap + (vertical ? leadingSpace : 0)

    Rectangle {
        visible: host.lifted
        z: -1
        anchors {
            fill: parent
            leftMargin: host.vertical ? 0 : -5
            rightMargin: host.vertical ? 0 : -5
            topMargin: host.vertical ? -5 : 4
            bottomMargin: host.vertical ? -5 : 4
        }
        radius: host.chipRadius
        color: host.chipColor
        // The outline is of the widget that is held. Let go, it slides to its place on the chip
        // alone: edit mode's outline is around that place at once, and is the only one.
        border.width: host.carried ? 2 : 0
        border.color: host.chipBorderColor
    }

    Rectangle {
        visible: host.divided
        x: host.dividerInset
        y: -(host.dividerGap + 1)
        width: host.width - x * 2
        height: 1
        color: host.dividerColor
    }
}
