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
        spacing: 4
        runs: panel.center
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
            fillBefore: true
            vertical: root.vertical
            edge: panel.edge
            zoneHovered: endArea.hovered
            shortened: root.shortened
        }
    }
}
