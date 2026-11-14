import QtQuick
import QtQuick.Layouts
import Impulse
import ri.services
import ri.modules.common
import ri.modules.common.widgets

/**
 * Edit mode's widget catalog: the overlay gives it the controller's `catalog` and shows it from
 * the toolbar's "Add widget". The user picks where the widget goes (desktop, bar, right
 * sidebar), then a widget; `picked` carries the type and the surface, and the overlay adds it
 * and reports back with `refused` when there was no room. Esc or a click outside emits
 * `dismiss`.
 */
WindowDialog {
    id: root
    property var catalog: []
    property string surface: "desktop"
    property bool refused: false
    readonly property var offered: catalog.filter(t => t[root.surface])

    signal picked(string type, string surface)

    backgroundWidth: 560
    focus: root.show
    onSurfaceChanged: root.refused = false

    RowLayout {
        Layout.fillWidth: true
        spacing: 12

        MaterialSymbol {
            text: "widgets"
            iconSize: Appearance.font.pixelSize.hugeass
            color: Appearance.colors.colOnSurface
        }
        WindowDialogTitle {
            Layout.fillWidth: true
            text: Translation.tr("Add widget")
        }
    }

    ConfigSelectionArray {
        currentValue: root.surface
        onSelected: newValue => root.surface = newValue
        options: [
            {
                displayName: Translation.tr("Desktop"),
                icon: "wallpaper",
                value: "desktop"
            },
            {
                displayName: Translation.tr("Bar"),
                icon: "toolbar",
                value: "panel"
            },
            {
                displayName: Translation.tr("Right sidebar"),
                icon: "side_navigation",
                value: "sidebar"
            }
        ]
    }

    StyledFlickable {
        id: flick
        Layout.fillWidth: true
        // As tall for every surface: the surface buttons stay under the pointer.
        implicitHeight: Math.min(5 * 52 + 4 * tiles.rowSpacing, root.height * 0.5)
        contentHeight: tiles.implicitHeight
        clip: true

        GridLayout {
            id: tiles
            width: flick.width
            columns: 3
            columnSpacing: 6
            rowSpacing: 6

            Repeater {
                model: root.offered
                delegate: RippleButton {
                    id: tile
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    implicitHeight: 52
                    focusPolicy: Qt.NoFocus
                    buttonRadius: Appearance.rounding.small
                    colBackground: Appearance.colors.colLayer3
                    colBackgroundHover: Appearance.colors.colLayer3Hover
                    onClicked: root.picked(tile.modelData.type, root.surface)

                    contentItem: RowLayout {
                        spacing: 10

                        MaterialSymbol {
                            Layout.leftMargin: 6
                            text: tile.modelData.icon
                            iconSize: Appearance.font.pixelSize.larger
                            color: Appearance.colors.colOnLayer3
                        }
                        StyledText {
                            Layout.fillWidth: true
                            text: Translation.tr(tile.modelData.name)
                            color: Appearance.colors.colOnLayer3
                            elide: Text.ElideRight
                        }
                    }
                }
            }
        }
    }

    StyledText {
        Layout.fillWidth: true
        text: root.refused ? Translation.tr("No room there. Remove a widget or pick another place.") : Translation.tr("The widget appears selected: drag it where you want it.")
        color: root.refused ? Appearance.colors.colError : Appearance.colors.colSubtext
        wrapMode: Text.Wrap
    }

    WindowDialogButtonRow {
        Layout.fillWidth: true

        Item {
            Layout.fillWidth: true
        }
        DialogButton {
            buttonText: Translation.tr("Close")
            onClicked: root.dismiss()
        }
    }
}
