import QtQuick
import QtQuick.Layouts
import ri
import ri.modules.common

// A run of a panel zone: one widget, or adjacent `pill` widgets on one shared group
// background drawn like the original's BarGroup. In the center zone, which has no free space of its own,
// a stretching run is as long as the original's center-side module (its stretching widgets take the rest),
// so the center zone lines up the way the original's middle section does.
Item {
    id: root
    required property var modelData
    property bool vertical: false
    property string edge: "top"
    property bool zoneHovered: false
    property int shortened: 0
    property bool center: false
    readonly property real centerSideLength: shortened === 2 ? Appearance.sizes.barCenterSideModuleWidthHellaShortened : shortened === 1 ? Appearance.sizes.barCenterSideModuleWidthShortened : Appearance.sizes.barCenterSideModuleWidth
    readonly property bool pill: modelData.pill
    // the original's BarGroup padding, unless the run's first widget asks for its own (vertical workspaces).
    readonly property real padding: pill ? ((widgets.count > 0 ? widgets.itemAt(0)?.item?.groupPadding : undefined) ?? (vertical ? 8 : 5)) : 0

    // A run whose widgets are all hidden lays out nothing, and takes no space or spacing.
    // A stretching run outside the center zone (a spacer) is free space even when empty.
    readonly property bool anyShown: {
        for (let i = 0; i < widgets.count; i++) {
            const item = widgets.itemAt(i)?.item;
            if (item && (item.shown ?? true))
                return true;
        }
        return false;
    }
    // The zone-wide position before which (`keepAfter`: after which) its widgets have room.
    property int cut: -1
    property bool keepAfter: false
    readonly property int count: widgets.count
    readonly property real spacing: vertical ? grid.rowSpacing : grid.columnSpacing
    function host(i) {
        return widgets.itemAt(i);
    }
    // The length the run cannot give up along the panel: its shown widgets' minimum lengths
    // with the spacing between them and its padding.
    readonly property real minimumLength: {
        let length = 0;
        let shown = 0;
        for (let i = 0; i < widgets.count; i++) {
            const host = widgets.itemAt(i);
            if (host?.item && (host.item.shown ?? true) && host.kept) {
                length += host.minimumLength + (vertical ? host.leadingSpace : 0);
                shown++;
            }
        }
        return shown > 0 ? length + (shown - 1) * spacing + padding * 2 : 0;
    }
    visible: (vertical ? grid.implicitHeight > 0 : grid.implicitWidth > 0) || (!center && modelData.stretch && anyShown)
    implicitWidth: vertical ? Appearance.sizes.baseVerticalBarWidth : center && modelData.stretch ? centerSideLength : grid.implicitWidth + padding * 2
    implicitHeight: vertical ? (grid.implicitHeight + padding * 2) : Appearance.sizes.baseBarHeight
    Layout.minimumWidth: vertical ? -1 : minimumLength
    Layout.minimumHeight: vertical ? minimumLength : -1
    Layout.fillWidth: !vertical && !center && modelData.stretch
    Layout.fillHeight: vertical && !center && modelData.stretch
    Layout.alignment: vertical ? Qt.AlignHCenter : Qt.AlignVCenter

    Rectangle {
        visible: root.pill
        anchors {
            fill: parent
            topMargin: root.vertical ? 0 : 4
            bottomMargin: root.vertical ? 0 : 4
            leftMargin: root.vertical ? 4 : 0
            rightMargin: root.vertical ? 4 : 0
        }
        color: Config.options?.bar.borderless ? "transparent" : Appearance.colors.colLayer1
        radius: Appearance.rounding.small
    }

    // the original's horizontal bar has its clock group (clock, utility buttons, battery) in a MouseArea
    // that toggles the right sidebar: a press no widget of the group takes lands there.
    MouseArea {
        anchors.fill: parent
        visible: {
            for (let i = 0; i < widgets.count; i++) {
                if (widgets.itemAt(i)?.item?.groupOpensRightSidebar)
                    return true;
            }
            return false;
        }
        onPressed: GlobalStates.sidebarRightOpen = !GlobalStates.sidebarRightOpen
    }

    GridLayout {
        id: grid
        columns: root.vertical ? 1 : -1
        anchors {
            // A pill run is the original's BarGroup (grid centered across the panel); a bare widget
            // sits in the zone's own cell, like the original's unwrapped bar modules.
            verticalCenter: root.vertical || !root.pill ? undefined : parent.verticalCenter
            horizontalCenter: root.vertical && root.pill ? parent.horizontalCenter : undefined
            left: root.vertical && root.pill ? undefined : parent.left
            right: root.vertical && root.pill ? undefined : parent.right
            top: root.vertical || !root.pill ? parent.top : undefined
            bottom: root.vertical || !root.pill ? parent.bottom : undefined
            margins: root.padding
        }
        columnSpacing: 4
        rowSpacing: 12

        Repeater {
            id: widgets
            model: root.modelData.entries
            delegate: PanelWidget {
                vertical: root.vertical
                edge: root.edge
                zoneHovered: root.zoneHovered
                shortened: root.shortened
                divided: root.vertical && root.pill && index > 0
                dividerGap: grid.rowSpacing
                dividerInset: Appearance.sizes.baseBarHeight / 3
                dividerColor: Appearance.colors.colOutlineVariant
                kept: root.cut < 0 || (root.keepAfter ? root.modelData.first + index > root.cut : root.modelData.first + index < root.cut)
            }
        }
    }
}
