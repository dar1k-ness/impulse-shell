import QtQuick
import QtQuick.Layouts
import Rustshell
import Rustshell.Hyprland
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets

/**
 * The desktop's right-click menu, as Plasma's: opens at the pointer on the background window,
 * closes on a pick, Esc or a click anywhere else (Hyprland focus grab).
 */
PopupWindow {
    id: root

    required property point position
    property real padding: Appearance.sizes.elevationMargin

    signal editRequested
    signal closed

    function close() {
        root.visible = false;
        root.closed();
    }

    visible: true
    color: "transparent"
    implicitWidth: menuBackground.implicitWidth + root.padding * 2
    implicitHeight: menuBackground.implicitHeight + root.padding * 2

    anchor {
        rect.x: root.position.x
        rect.y: root.position.y
        rect.width: 1
        rect.height: 1
        edges: Edges.Top | Edges.Left
        gravity: Edges.Bottom | Edges.Right
        adjustment: PopupAdjustment.Flip | PopupAdjustment.Slide
    }

    HyprlandFocusGrab {
        active: root.visible
        windows: [root]
        onCleared: root.close()
    }

    Item {
        anchors.fill: parent
        focus: true
        Keys.onEscapePressed: root.close()

        StyledRectangularShadow {
            target: menuBackground
        }

        Rectangle {
            id: menuBackground
            readonly property real padding: 4
            anchors {
                fill: parent
                margins: root.padding
            }
            color: Appearance.colors.colLayer0
            radius: Appearance.rounding.windowRounding
            border.width: 1
            border.color: Appearance.colors.colLayer0Border
            clip: true
            implicitWidth: entries.implicitWidth + menuBackground.padding * 2
            implicitHeight: entries.implicitHeight + menuBackground.padding * 2

            ColumnLayout {
                id: entries
                anchors {
                    fill: parent
                    margins: menuBackground.padding
                }
                spacing: 0

                DesktopMenuEntry {
                    symbol: "edit"
                    label: Translation.tr("Enter edit mode")
                    onTriggered: {
                        root.editRequested();
                        root.close();
                    }
                }
                DesktopMenuEntry {
                    symbol: "wallpaper"
                    label: Translation.tr("Change wallpaper")
                    onTriggered: {
                        GlobalStates.wallpaperSelectorOpen = true;
                        root.close();
                    }
                }
            }
        }
    }
}
