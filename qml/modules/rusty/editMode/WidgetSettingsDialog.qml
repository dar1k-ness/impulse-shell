import QtQuick
import QtQuick.Layouts
import Impulse
import ri.services
import ri.modules.common
import ri.modules.common.widgets

/**
 * Edit mode's settings of one widget: a panel at the side of the overlay, the side the widget
 * is not on, so the widget stays in view and shows each change as it is made. The overlay
 * sets `connector` and `widgetId` (the selected widget: selecting another shows that one's
 * settings) and opens it from the gear or a double click. Its groups, in the manner of a
 * settings page: the options of the widget type's schema, then its size (a desktop widget's
 * width and height within the type's limits, a sidebar widget's columns) and where a bar
 * widget sits (`place`), changed through the overlay. Every accepted change is written to
 * layout.toml as one undo step; rejected input stays in its field with the reason underneath.
 * Esc, the close button and Done emit `dismiss`; so does the selection going away.
 */
Item {
    id: root
    property string connector
    property string widgetId
    // The surface the widget is on: "desktop", "panel", "sidebar".
    property string kind: ""
    // Where the widget sits (the controller's `selectedPlace`): a bar widget's zone and width,
    // a sidebar widget's span, a desktop widget's size with its limits. The overlay applies a
    // pick and sets `refused` when it had no room.
    property var place: null
    property bool refused: false
    // The widget's outline in the overlay, which the panel keeps off; `busy` while it is
    // dragged, when the panel stays where it is.
    property var avoid: null
    property bool busy: false
    property bool show: false
    signal zonePicked(string zone)
    signal widthPicked(string mode)
    signal spanPicked(int size)
    signal sizePicked(real width, real height)
    signal dismiss

    readonly property real panelWidth: 400
    readonly property real margin: Appearance.sizes.elevationMargin * 2
    // The panel is on the right, or on the left while the widget is where it would be.
    property bool onLeft: false
    function pickSide() {
        const f = root.avoid;
        if (f === null || root.busy)
            return;
        const underRight = f.x + f.width > root.width - root.margin - root.panelWidth;
        const underLeft = f.x < root.margin + root.panelWidth;
        if (!root.onLeft && underRight && !underLeft)
            root.onLeft = true;
        else if (root.onLeft && underLeft && !underRight)
            root.onLeft = false;
    }
    onAvoidChanged: root.pickSide()
    onBusyChanged: root.pickSide()
    onShowChanged: root.pickSide()
    onWidgetIdChanged: {
        root.refused = false;
        if (root.widgetId === "" && root.show)
            root.dismiss();
    }

    visible: panel.opacity > 0
    focus: root.show
    Keys.onPressed: event => {
        if (event.key === Qt.Key_Escape) {
            root.dismiss();
            event.accepted = true;
        }
    }

    WidgetSettings {
        id: settings
        connector: root.connector
        widgetId: root.widgetId
    }

    StyledRectangularShadow {
        target: panel
        opacity: panel.opacity
    }
    Rectangle {
        id: panel
        readonly property real padding: 20
        readonly property real restX: root.onLeft ? root.margin : root.width - root.margin - panel.width
        width: root.panelWidth
        height: Math.min(keyScope.implicitHeight + panel.padding * 2, root.height - root.margin * 2)
        // Slides in from its side, and across when the widget goes under it.
        x: Math.round(root.show ? panel.restX : panel.restX + (root.onLeft ? -40 : 40))
        y: Math.round((root.height - panel.height) / 2)
        radius: Appearance.rounding.large
        color: Appearance.m3colors.m3surfaceContainerHigh
        opacity: root.show ? 1 : 0
        Behavior on x {
            animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
        }
        Behavior on opacity {
            animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
        }
        Behavior on height {
            animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
        }

        // A press on the panel is not one on the widgets under it.
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
            hoverEnabled: true
        }

        // Ancestor of every row and button, so Ctrl+Z / Ctrl+Shift+Z reach it from whichever of
        // them holds the focus; as a focus scope it keeps active focus when a row lets go of it.
        FocusScope {
            id: keyScope
            anchors {
                fill: parent
                margins: panel.padding
            }
            implicitHeight: body.implicitHeight
            focus: true
            Keys.onPressed: event => {
                if (!(event.modifiers & Qt.ControlModifier))
                    return;
                if (event.key === Qt.Key_Z && (event.modifiers & Qt.ShiftModifier) || event.key === Qt.Key_Y) {
                    settings.redo();
                    event.accepted = true;
                } else if (event.key === Qt.Key_Z) {
                    settings.undo();
                    event.accepted = true;
                }
            }

            ColumnLayout {
                id: body
                anchors.fill: parent
                spacing: 16

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 12

                    Rectangle {
                        implicitWidth: 44
                        implicitHeight: 44
                        radius: 22
                        color: Appearance.colors.colPrimaryContainer
                        MaterialSymbol {
                            anchors.centerIn: parent
                            text: settings.icon
                            iconSize: 24
                            color: Appearance.colors.colOnPrimaryContainer
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        StyledText {
                            Layout.fillWidth: true
                            text: settings.title
                            elide: Text.ElideRight
                            color: Appearance.colors.colOnSurface
                            font.pixelSize: Appearance.font.pixelSize.larger
                            font.weight: Font.Medium
                        }
                        StyledText {
                            Layout.fillWidth: true
                            text: !settings.valid ? Translation.tr("This widget is no longer in the layout") : root.kind === "desktop" ? Translation.tr("On the desktop") : root.kind === "panel" ? Translation.tr("On the bar") : root.kind === "sidebar" ? Translation.tr("In the right sidebar") : ""
                            elide: Text.ElideRight
                            color: Appearance.colors.colSubtext
                            font.pixelSize: Appearance.font.pixelSize.smaller
                        }
                    }
                    RippleButton {
                        implicitWidth: 36
                        implicitHeight: 36
                        buttonRadius: Appearance.rounding.full
                        // The icon it shows.
                        text: "close"
                        onClicked: root.dismiss()
                        contentItem: MaterialSymbol {
                            anchors.centerIn: parent
                            horizontalAlignment: Text.AlignHCenter
                            text: "close"
                            iconSize: 22
                            color: Appearance.colors.colOnSurfaceVariant
                        }
                        StyledToolTip {
                            text: Translation.tr("Close")
                        }
                    }
                }

                StyledFlickable {
                    id: flick
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    visible: settings.valid
                    implicitHeight: groups.implicitHeight
                    contentHeight: groups.implicitHeight
                    clip: true

                    ColumnLayout {
                        id: groups
                        width: flick.width
                        spacing: 16

                        SettingsGroup {
                            title: Translation.tr("Options")
                            shown: settings.fields.length > 0

                            Repeater {
                                // A count, not the field list: rows stay (with their focus) when values change.
                                model: settings.fields.length
                                delegate: WidgetSettingsField {
                                    required property int index
                                    Layout.fillWidth: true
                                    field: settings.fields[index]
                                    onChanged: value => settings.setValue(field.key, value)
                                    onReset: settings.resetValue(field.key)
                                }
                            }
                        }

                        SettingsGroup {
                            title: Translation.tr("Size")
                            shown: root.place?.sizeWidth !== undefined

                            SettingsSpinRow {
                                title: Translation.tr("Width")
                                subtitle: Translation.tr("From %1 to %2").arg(root.place?.minWidth ?? 0).arg(root.place?.maxWidth ?? 0)
                                value: root.place?.sizeWidth ?? 0
                                from: root.place?.minWidth ?? 0
                                to: root.place?.maxWidth ?? 0
                                stepSize: 10
                                unit: "px"
                                onUserChanged: value => root.sizePicked(value, root.place.sizeHeight)
                            }
                            SettingsSpinRow {
                                title: Translation.tr("Height")
                                subtitle: Translation.tr("From %1 to %2").arg(root.place?.minHeight ?? 0).arg(root.place?.maxHeight ?? 0)
                                value: root.place?.sizeHeight ?? 0
                                from: root.place?.minHeight ?? 0
                                to: root.place?.maxHeight ?? 0
                                stepSize: 10
                                unit: "px"
                                onUserChanged: value => root.sizePicked(root.place.sizeWidth, value)
                            }
                        }

                        SettingsGroup {
                            title: Translation.tr("Placement")
                            shown: root.place?.zone !== undefined

                            SettingsSegmentedRow {
                                title: Translation.tr("Bar zone")
                                options: [
                                    { "label": Translation.tr("Start"), "value": "start" },
                                    { "label": Translation.tr("Center"), "value": "center" },
                                    { "label": Translation.tr("End"), "value": "end" }
                                ]
                                currentValue: root.place?.zone ?? ""
                                onSelected: value => root.zonePicked(value)
                            }
                            SettingsSegmentedRow {
                                shown: root.place?.width !== undefined
                                title: Translation.tr("Width")
                                options: [
                                    { "label": Translation.tr("Fit content"), "value": "auto" },
                                    { "label": Translation.tr("Stretch"), "value": "stretch" }
                                ]
                                currentValue: root.place?.width ?? ""
                                onSelected: value => root.widthPicked(value)
                            }
                        }

                        SettingsGroup {
                            title: Translation.tr("Size")
                            shown: (root.place?.columns ?? 0) > 1

                            SettingsSegmentedRow {
                                title: Translation.tr("Columns taken")
                                options: Array.from({ "length": root.place?.columns ?? 0 }, (_, i) => ({ "label": String(i + 1), "value": i + 1 }))
                                currentValue: root.place?.span ?? 0
                                onSelected: value => root.spanPicked(value)
                            }
                        }

                        StyledText {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            visible: root.refused
                            text: Translation.tr("No room there. Remove a widget or pick another place.")
                            color: Appearance.colors.colError
                            font.pixelSize: Appearance.font.pixelSize.smaller
                            wrapMode: Text.Wrap
                        }

                        StyledText {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            visible: settings.fields.length === 0 && root.place === null
                            text: Translation.tr("This widget has no settings")
                            color: Appearance.colors.colSubtext
                            wrapMode: Text.Wrap
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 4

                    RippleButton {
                        implicitWidth: 36
                        implicitHeight: 36
                        buttonRadius: Appearance.rounding.full
                        enabled: settings.canUndo
                        onClicked: settings.undo()
                        contentItem: MaterialSymbol {
                            anchors.centerIn: parent
                            horizontalAlignment: Text.AlignHCenter
                            text: "undo"
                            iconSize: 22
                            color: parent.enabled ? Appearance.colors.colOnSurfaceVariant : Appearance.m3colors.m3outline
                        }
                        StyledToolTip {
                            text: Translation.tr("Undo (Ctrl+Z)")
                        }
                    }
                    RippleButton {
                        implicitWidth: 36
                        implicitHeight: 36
                        buttonRadius: Appearance.rounding.full
                        enabled: settings.canRedo
                        onClicked: settings.redo()
                        contentItem: MaterialSymbol {
                            anchors.centerIn: parent
                            horizontalAlignment: Text.AlignHCenter
                            text: "redo"
                            iconSize: 22
                            color: parent.enabled ? Appearance.colors.colOnSurfaceVariant : Appearance.m3colors.m3outline
                        }
                        StyledToolTip {
                            text: Translation.tr("Redo (Ctrl+Shift+Z)")
                        }
                    }
                    Item {
                        Layout.fillWidth: true
                    }
                    DialogButton {
                        buttonText: Translation.tr("Done")
                        onClicked: root.dismiss()
                    }
                }
            }
        }
    }
}
