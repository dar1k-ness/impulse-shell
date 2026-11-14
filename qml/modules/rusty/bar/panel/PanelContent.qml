import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.rusty.bar as Bar
import Impulse

// The bar content region of a panel from the layout (layout.toml): three zones of registry
// widgets along the panel, horizontal on a top/bottom edge and vertical on a left/right one.
// The start zone scrolls brightness and opens the left sidebar, the end zone scrolls volume
// and opens the right sidebar, as in the original's BarContent / VerticalBarContent.
Item {
    id: root

    property var screen: root.QsWindow.window?.screen
    property var brightnessMonitor: Brightness.getMonitorForScreen(screen)
    readonly property bool vertical: panel.vertical
    readonly property int shortened: vertical ? 0 : (Appearance.sizes.barHellaShortenScreenWidthThreshold >= screen?.width) ? 2 : (Appearance.sizes.barShortenScreenWidthThreshold >= screen?.width) ? 1 : 0
    readonly property real edgePadding: Appearance.rounding.screenRounding

    PanelContainer {
        id: panel
        connector: root.screen?.name ?? ""
        // the original's bar edge, for a panel the layout puts there: the original's settings keep choosing it.
        rustyBarEdge: Config.options.bar.vertical ? (Config.options.bar.bottom ? "right" : "left")
            : (Config.options.bar.bottom ? "bottom" : "top")
        // Where edit mode may drop one more widget. The center zone grows to both sides, out
        // of the free length the start and end zones keep.
        startRoom: startZone.room
        centerRoom: Math.max(0, Math.min(centerZone.freeLength, 2 * Math.min(startZone.freeLength, endZone.freeLength)) - centerZone.overhead)
        endRoom: endZone.room
        // What a drop of each of the monitor's widgets charges a zone, as PanelZone.fit
        // charges the widget once it is there: its least length, with its leading margin on a
        // vertical bar; nothing for one that does not show. The panel's own from its zones,
        // the others from their probes.
        panelLengths: {
            const lengths = {};
            for (const zone of [startZone, centerZone, endZone]) {
                const own = zone.lengths;
                for (const id in own)
                    lengths[id] = own[id];
            }
            for (let i = 0; i < probeHosts.count; i++) {
                const host = probeHosts.itemAt(i);
                if (host?.item)
                    lengths[host.entry.id] = (host.item.shown ?? true)
                        ? host.minimumLength + (root.vertical ? host.leadingSpace : 0) : 0;
            }
            return lengths;
        }
    }

    // While edit mode is on, the monitor's other widgets that may go onto this panel, laid out
    // in their panel form but not shown, for their lengths.
    Item {
        visible: false
        Repeater {
            id: probeHosts
            model: panel.probes
            PanelWidget {
                probe: true
                vertical: root.vertical
                edge: panel.edge
                shortened: root.shortened
            }
        }
    }

    // Background shadow. `bar.floatStyleShadow` turns it off on a horizontal bar only, as in
    // the original (its BarContent reads the option, its VerticalBarContent does not).
    Loader {
        active: Config.options.bar.showBackground && Config.options.bar.cornerStyle === 1
            && (root.vertical || Config.options.bar.floatStyleShadow)
        anchors.fill: barBackground
        sourceComponent: StyledRectangularShadow {
            anchors.fill: undefined
            target: barBackground
        }
    }
    // Background
    Rectangle {
        id: barBackground
        anchors {
            fill: parent
            margins: Config.options.bar.cornerStyle === 1 ? (Appearance.sizes.hyprlandGapsOut) : 0
        }
        color: Config.options.bar.showBackground ? Appearance.colors.colLayer0 : "transparent"
        radius: Config.options.bar.cornerStyle === 1 ? Appearance.rounding.windowRounding : 0
        border.width: Config.options.bar.cornerStyle === 1 ? 1 : 0
        border.color: Appearance.colors.colLayer0Border
    }

    FocusedScrollMouseArea { // Start zone | scroll to change brightness
        id: startArea
        anchors {
            left: parent.left
            top: parent.top
            right: root.vertical ? parent.right : centerZone.left
            bottom: root.vertical ? centerZone.top : parent.bottom
        }
        implicitWidth: startZone.implicitWidth
        implicitHeight: startZone.implicitHeight

        onScrollDown: Brightness.decreaseBrightness()
        onScrollUp: Brightness.increaseBrightness()
        onMovedAway: GlobalStates.osdBrightnessOpen = false
        onPressed: event => {
            if (event.button === Qt.LeftButton)
                GlobalStates.sidebarLeftOpen = !GlobalStates.sidebarLeftOpen;
        }

        Bar.ScrollHint {
            visible: !root.vertical
            reveal: startArea.hovered
            icon: Hyprsunset.gamma === 100 ? "light_mode" : "wb_twilight"
            tooltipText: Translation.tr("Scroll to change brightness")
            side: "left"
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
        }

        PanelZone {
            id: startZone
            anchors {
                fill: parent
                leftMargin: root.vertical ? 0 : root.edgePadding
                topMargin: root.vertical ? Appearance.sizes.hyprlandGapsOut : 0
            }
            spacing: root.vertical ? 10 : 0
            runs: panel.start
            label: (root.screen?.name ?? "") + " start"
            fillAfter: true
            vertical: root.vertical
            edge: panel.edge
            zoneHovered: startArea.hovered
            shortened: root.shortened
        }
    }

    PanelZone {
        id: centerZone
        anchors.centerIn: parent
        // Sized to its runs, within the bar's length between its rounded ends, leaving either
        // side zone its widget at the bar's edge: when the bar is short, the center gives way.
        available: (root.vertical ? root.height : root.width) - 2 * Math.max(root.edgePadding, (root.vertical ? Appearance.sizes.hyprlandGapsOut : root.edgePadding) + startZone.edgeLength, root.edgePadding + endZone.edgeLength)
        width: root.vertical ? implicitWidth : Math.min(implicitWidth, available)
        height: root.vertical ? Math.min(implicitHeight, available) : implicitHeight
        spacing: 4
        runs: panel.center
        label: (root.screen?.name ?? "") + " center"
        center: true
        vertical: root.vertical
        edge: panel.edge
        shortened: root.shortened
    }

    FocusedScrollMouseArea { // End zone | scroll to change volume
        id: endArea
        anchors {
            right: parent.right
            bottom: parent.bottom
            left: root.vertical ? parent.left : centerZone.right
            top: root.vertical ? centerZone.bottom : parent.top
        }
        implicitWidth: endZone.implicitWidth
        implicitHeight: endZone.implicitHeight

        onScrollDown: Audio.decrementVolume()
        onScrollUp: Audio.incrementVolume()
        onMovedAway: GlobalStates.osdVolumeOpen = false
        onPressed: event => {
            if (event.button === Qt.LeftButton)
                GlobalStates.sidebarRightOpen = !GlobalStates.sidebarRightOpen;
        }

        Bar.ScrollHint {
            visible: !root.vertical
            reveal: endArea.hovered
            icon: "volume_up"
            tooltipText: Translation.tr("Scroll to change volume")
            side: "right"
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
        }

        PanelZone {
            id: endZone
            anchors {
                fill: parent
                rightMargin: root.vertical ? 0 : root.edgePadding
                bottomMargin: root.vertical ? root.edgePadding : 0
            }
            spacing: root.vertical ? 4 : 5
            runs: panel.end
            label: (root.screen?.name ?? "") + " end"
            fillBefore: true
            vertical: root.vertical
            edge: panel.edge
            zoneHovered: endArea.hovered
            shortened: root.shortened
        }
    }
}
