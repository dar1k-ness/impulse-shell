import QtQuick
import QtQuick.Layouts
import Impulse
import ri.services
import ri.modules.common
import ri.modules.common.widgets

/**
 * Edit mode's widget catalog: the overlay gives it the controller's `catalog` and shows it from
 * the toolbar's "Add widget". A sheet that slides in from the left edge and leaves the desktop
 * in view. The user picks where the widget goes (desktop, bar, right sidebar), then a widget:
 * a click on it emits `picked` with the type and the surface, and the overlay adds it and
 * reports back with `refused` when there was no room. A desktop widget can also be dragged out
 * of the sheet to where it should stand: `carryStarted` when the drag begins, `carryMoved`
 * while it goes on, `carryEnded` where it is let go (all in this item's coordinates). The rest
 * of the overlay stays in use beside the open sheet; Esc or "Close" emits `dismiss`.
 */
Item {
    id: root
    property bool show: false
    property var catalog: []
    property string surface: "desktop"
    property bool refused: false
    readonly property var offered: catalog.filter(t => t[root.surface])
    // A widget is being dragged out of the sheet, which gets out of its way.
    property bool carrying: false

    signal picked(string type, string surface)
    signal dismiss
    signal carryStarted(string type, real x, real y)
    signal carryMoved(real x, real y)
    signal carryEnded(real x, real y)

    focus: root.show
    onSurfaceChanged: root.refused = false
    // Out of sight while a widget is carried, it still gets the pointer the carry began on.
    visible: sheet.opacity > 0 || root.carrying
    Keys.onPressed: event => {
        if (event.key === Qt.Key_Escape) {
            root.dismiss();
            event.accepted = true;
        }
    }

    StyledRectangularShadow {
        target: sheet
        opacity: sheet.opacity
    }
    Rectangle {
        id: sheet
        readonly property real margin: Appearance.sizes.elevationMargin * 2
        readonly property bool out: root.show && !root.carrying
        width: 380
        height: Math.min(root.height - 2 * 72, column.implicitHeight + 2 * radius)
        y: Math.round((root.height - sheet.height) / 2)
        x: sheet.out ? sheet.margin : -sheet.width
        opacity: sheet.out ? 1 : 0
        radius: Appearance.rounding.large
        color: Appearance.m3colors.m3surfaceContainerHigh
        Behavior on x {
            animation: Appearance.animation.elementMove.numberAnimation.createObject(this)
        }
        Behavior on opacity {
            animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
        }

        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
        }

        ColumnLayout {
            id: column
            anchors {
                fill: parent
                margins: sheet.radius
            }
            spacing: 16

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
                implicitHeight: Math.min(4 * 92 + 3 * tiles.rowSpacing, root.height * 0.5)
                contentHeight: tiles.implicitHeight
                clip: true

                GridLayout {
                    id: tiles
                    width: flick.width
                    columns: 2
                    columnSpacing: 8
                    rowSpacing: 8

                    Repeater {
                        model: root.offered
                        delegate: RippleButton {
                            id: tile
                            required property var modelData
                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            implicitHeight: 92
                            focusPolicy: Qt.NoFocus
                            buttonRadius: Appearance.rounding.normal
                            colBackground: carry.containsMouse ? Appearance.colors.colLayer3Hover : Appearance.colors.colLayer3
                            colBackgroundHover: Appearance.colors.colLayer3Hover

                            // A click picks the widget; a desktop widget dragged off its tile
                            // is carried to where it is let go.
                            MouseArea {
                                id: carry
                                anchors.fill: parent
                                hoverEnabled: true
                                // The list does not take a drag begun on a tile: it carries the widget.
                                preventStealing: true
                                cursorShape: root.surface === "desktop" ? Qt.OpenHandCursor : Qt.PointingHandCursor
                                property real fromX: 0
                                property real fromY: 0
                                function here(mouse) {
                                    return carry.mapToItem(root, mouse.x, mouse.y);
                                }
                                onPressed: mouse => {
                                    carry.fromX = mouse.x;
                                    carry.fromY = mouse.y;
                                }
                                onPositionChanged: mouse => {
                                    const p = carry.here(mouse);
                                    if (root.carrying) {
                                        root.carryMoved(p.x, p.y);
                                    } else if (carry.pressed && root.surface === "desktop" && Math.abs(mouse.x - carry.fromX) + Math.abs(mouse.y - carry.fromY) > 12) {
                                        root.carrying = true;
                                        root.carryStarted(tile.modelData.type, p.x, p.y);
                                    }
                                }
                                onReleased: mouse => {
                                    const p = carry.here(mouse);
                                    if (root.carrying) {
                                        root.carrying = false;
                                        root.carryEnded(p.x, p.y);
                                    } else if (carry.containsMouse) {
                                        root.picked(tile.modelData.type, root.surface);
                                    }
                                }
                                onCanceled: {
                                    if (root.carrying) {
                                        root.carrying = false;
                                        root.carryEnded(-1, -1);
                                    }
                                }
                            }

                            contentItem: ColumnLayout {
                                spacing: 6

                                Item {
                                    Layout.fillHeight: true
                                }
                                MaterialSymbol {
                                    Layout.alignment: Qt.AlignHCenter
                                    text: tile.modelData.icon
                                    iconSize: Appearance.font.pixelSize.hugeass
                                    color: Appearance.colors.colOnLayer3
                                }
                                StyledText {
                                    Layout.fillWidth: true
                                    horizontalAlignment: Text.AlignHCenter
                                    text: Translation.tr(tile.modelData.name)
                                    color: Appearance.colors.colOnLayer3
                                    elide: Text.ElideRight
                                }
                                Item {
                                    Layout.fillHeight: true
                                }
                            }
                        }
                    }
                }
            }

            StyledText {
                Layout.fillWidth: true
                text: root.refused ? Translation.tr("No room there. Remove a widget or pick another place.") : Translation.tr("Click a widget to add it, or drag it out to where you want it.")
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
    }
}
