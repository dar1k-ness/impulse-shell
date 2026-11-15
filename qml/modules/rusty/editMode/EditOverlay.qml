import QtQuick
import QtQuick.Layouts
import Rustshell
import Rustshell.Wayland
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import Impulse

/**
 * The edit overlay of one monitor: a fullscreen layer surface on the overlay layer, so a drag
 * can go from the panel to the desktop or the sidebar within one surface. It forwards the
 * pointer and keys to the EditController and draws what it publishes: the outline of the
 * widget the pointer is over, the selection with its resize handles (each corner and side of a
 * desktop widget, the two ends of a panel's or sidebar's), the drag ghost, the size a resize
 * gives and whether it is the widget's limit, snapping guides and the drop marker.
 */
PanelWindow {
    id: root

    WlrLayershell.namespace: "rustshell:editMode"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: controller.active ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }

    EditController {
        id: controller
        connector: root.screen?.name ?? ""
    }

    // A desktop widget that places itself by the wallpaper and is put somewhere by hand stays
    // there: the option it places itself by (widgets.toml `placedBy`) becomes the original's "free".
    // A sidebar measures a widget only once it lays it out: one the catalog added that turns
    // out to have no room (the sidebar would hide it) is taken back, and the catalog says so.
    property string addedToSidebar: ""
    readonly property bool addedOverflows: addedToSidebar !== "" && controller.selectedId === addedToSidebar && (controller.selectedPlace?.overflows ?? false)
    onAddedOverflowsChanged: {
        if (addedOverflows)
            Qt.callLater(root.takeBackAdded);
    }
    function takeBackAdded() {
        if (!addedOverflows)
            return;
        controller.undo();
        catalogDialog.open();
        catalogDialog.item.surface = "sidebar";
        catalogDialog.item.refused = true;
    }

    function placedByHand() {
        if (controller.selectedKind !== "desktop")
            return;
        const type = controller.frames.find(f => f.selected)?.type ?? "";
        const path = controller.catalog.find(t => t.type === type)?.placedBy ?? "";
        if (path === "")
            return;
        const keys = path.split(".");
        let v = Config.options;
        for (let i = 0; i < keys.length && v !== undefined && v !== null; i++)
            v = v[keys[i]];
        if (v !== undefined && v !== null && v !== "free")
            Config.setNestedValue(path, "free");
    }

    // Part of the focus grab: a press on the overlay is not a click outside the right
    // sidebar, which would close it (and take keyboard focus with the grab).
    Component.onCompleted: GlobalFocusGrab.addPinned(root)
    Component.onDestruction: GlobalFocusGrab.removePinned(root)

    // Whether the overlay is on screen: it fades in once its first frames are out (making it
    // takes longer than a frame, and a fade started with it would be over before it is seen)
    // and fades out when the edit mode ends; EditMode.qml unloads it once `gone`.
    property int framesShown: 0
    readonly property bool shown: controller.active && root.framesShown >= 6
    readonly property bool gone: !controller.active && content.opacity === 0
    FrameAnimation {
        running: root.framesShown < 6
        onTriggered: root.framesShown++
    }

    Item {
        id: content
        anchors.fill: parent
        opacity: root.shown ? 1 : 0
        Behavior on opacity {
            animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
        }

        Rectangle {
            anchors.fill: parent
            color: ColorUtils.transparentize(Appearance.m3colors.m3scrim, 0.85)
        }

        MouseArea {
            id: input
            anchors.fill: parent
            // Fading out after the edit mode ended, the overlay takes no more input.
            enabled: controller.active
            focus: true
            acceptedButtons: Qt.LeftButton
            hoverEnabled: true
            // The pointer says what a press would do: pull the handle it is on, or pick up the
            // widget it is over.
            readonly property int handleShape: ({
                    "left": Qt.SizeHorCursor,
                    "right": Qt.SizeHorCursor,
                    "top": Qt.SizeVerCursor,
                    "bottom": Qt.SizeVerCursor,
                    "top-left": Qt.SizeFDiagCursor,
                    "bottom-right": Qt.SizeFDiagCursor,
                    "top-right": Qt.SizeBDiagCursor,
                    "bottom-left": Qt.SizeBDiagCursor
                })[controller.handle] ?? Qt.ArrowCursor
            cursorShape: controller.handle !== "" ? input.handleShape : !controller.dragging ? (controller.hovered !== null || input.overSelected ? Qt.OpenHandCursor : Qt.ArrowCursor) : (controller.ghost?.valid ?? true) ? Qt.ClosedHandCursor : Qt.ForbiddenCursor
            readonly property bool overSelected: {
                const f = controller.selectedFrame;
                return f !== null && input.containsMouse && input.mouseX >= f.x && input.mouseX <= f.x + f.width && input.mouseY >= f.y && input.mouseY <= f.y + f.height;
            }
            onPressed: mouse => {
                // Keys go to the overlay again after a press on it (the settings panel had them).
                input.forceActiveFocus();
                controller.press(mouse.x, mouse.y);
            }
            // A double click on a widget opens its settings, once it is let go without having
            // carried the widget anywhere.
            property bool doubled: false
            onDoubleClicked: input.doubled = true
            onPositionChanged: mouse => controller.move(mouse.x, mouse.y)
            onReleased: mouse => {
                const carried = controller.dragging;
                if (controller.release(mouse.x, mouse.y))
                    root.placedByHand();
                else if (input.doubled && !carried && controller.selectedId !== "")
                    settingsDialog.open();
                input.doubled = false;
            }
            onCanceled: {
                input.doubled = false;
                controller.cancel();
            }
            Keys.onPressed: event => {
                // Esc closes the settings panel before it leaves edit mode.
                if (event.key === Qt.Key_Escape && settingsDialog.shown && !controller.dragging) {
                    settingsDialog.item.dismiss();
                    event.accepted = true;
                    return;
                }
                // Esc leaves the edit mode unless it drops a drag (EditController): leaving so backs
                // out of a style review (services/StyleReview.qml), told before the controller
                // turns the edit mode off.
                const leaves = event.key === Qt.Key_Escape && controller.active && !controller.dragging;
                if (leaves)
                    StyleReview.leaving(true);
                event.accepted = controller.key(event.key, event.modifiers);
                if (leaves && !event.accepted)
                    StyleReview.leaving(false);
                if (event.accepted && [Qt.Key_Left, Qt.Key_Right, Qt.Key_Up, Qt.Key_Down].includes(event.key))
                    root.placedByHand();
            }
        }

        // The container a drag would drop into; none is outlined where the widget cannot go.
        Rectangle {
            visible: controller.target !== null && (controller.ghost?.valid ?? false)
            x: controller.target?.x ?? 0
            y: controller.target?.y ?? 0
            width: controller.target?.width ?? 0
            height: controller.target?.height ?? 0
            color: "transparent"
            border.width: 2
            border.color: ColorUtils.transparentize(Appearance.colors.colPrimary, 0.5)
            radius: Appearance.rounding.small
        }

        // The widget the pointer is over, outlined softly; the others are left as they look, so a
        // row of them (the bar) does not turn into a row of boxes. It keeps its last place while
        // it fades out.
        Rectangle {
            id: hoverOutline
            property var rect: null
            readonly property var now: controller.dragging ? null : controller.hovered
            onNowChanged: {
                if (hoverOutline.now !== null)
                    hoverOutline.rect = hoverOutline.now;
            }
            x: (hoverOutline.rect?.x ?? 0) - 1
            y: (hoverOutline.rect?.y ?? 0) - 1
            width: (hoverOutline.rect?.width ?? 0) + 2
            height: (hoverOutline.rect?.height ?? 0) + 2
            radius: Appearance.rounding.small + 1
            color: ColorUtils.transparentize(Appearance.colors.colOnSurface, 0.92)
            border.width: 1
            border.color: ColorUtils.transparentize(Appearance.colors.colOnSurface, 0.45)
            opacity: hoverOutline.now !== null ? 1 : 0
            visible: hoverOutline.opacity > 0
            Behavior on opacity {
                NumberAnimation {
                    duration: 120
                    easing.type: Easing.OutCubic
                }
            }
        }

        // The selected widget: its outline, around the widget and not over its edge (a border lies
        // inside its rectangle, and a widget without margins paints up to its own edge), and the
        // handles a resize is pulled by. While a handle is pulled the outline is the size the
        // resize would give; while a panel's or the sidebar's widget is carried it dims where the
        // widget was. A desktop widget is carried itself, so its outline is away until it lands.
        Item {
            id: selection
            property var rect: null
            readonly property var now: controller.selectedFrame
            onNowChanged: {
                if (selection.now !== null)
                    selection.rect = selection.now;
            }
            readonly property bool resizing: controller.dragging && controller.handle !== ""
            readonly property bool onDesktop: (selection.rect?.kind ?? "") === "desktop"
            readonly property bool carried: controller.dragging && !selection.resizing
            // Clear of a desktop widget, as Plasma's handles are; tight on a panel's.
            readonly property real outset: selection.onDesktop ? 6 : 2
            // Which handles it has: all eight, the two ends of a row, the two ends of a column.
            readonly property string handleSet: !(selection.rect?.resizable ?? false) ? "" : selection.rect.kind === "desktop" ? "all" : selection.rect.kind === "panel" && selection.rect.vertical ? "column" : "row"
            x: (selection.rect?.x ?? 0) - selection.outset
            y: (selection.rect?.y ?? 0) - selection.outset
            width: (selection.rect?.width ?? 0) + selection.outset * 2
            height: (selection.rect?.height ?? 0) + selection.outset * 2
            opacity: selection.now === null ? 0 : !selection.carried ? 1 : selection.onDesktop ? 0 : 0.4
            visible: selection.opacity > 0
            Behavior on opacity {
                NumberAnimation {
                    // It comes back once the widget has slid to its place.
                    duration: selection.carried ? 80 : 220
                    easing.type: selection.carried ? Easing.OutCubic : Easing.InCubic
                }
            }

            Rectangle {
                anchors.fill: parent
                radius: Appearance.rounding.small + selection.outset
                color: ColorUtils.transparentize(Appearance.colors.colPrimary, 0.88)
                border.width: 2
                border.color: Appearance.colors.colPrimary
            }

            Repeater {
                model: selection.handleSet === "all" ? [
                    {"name": "top-left", "fx": 0, "fy": 0},
                    {"name": "top", "fx": 0.5, "fy": 0},
                    {"name": "top-right", "fx": 1, "fy": 0},
                    {"name": "right", "fx": 1, "fy": 0.5},
                    {"name": "bottom-right", "fx": 1, "fy": 1},
                    {"name": "bottom", "fx": 0.5, "fy": 1},
                    {"name": "bottom-left", "fx": 0, "fy": 1},
                    {"name": "left", "fx": 0, "fy": 0.5}
                ] : selection.handleSet === "row" ? [
                    {"name": "left", "fx": 0, "fy": 0.5},
                    {"name": "right", "fx": 1, "fy": 0.5}
                ] : selection.handleSet === "column" ? [
                    {"name": "top", "fx": 0.5, "fy": 0},
                    {"name": "bottom", "fx": 0.5, "fy": 1}
                ] : []

                delegate: Rectangle {
                    id: grip
                    required property var modelData
                    readonly property bool corner: grip.modelData.fx !== 0.5 && grip.modelData.fy !== 0.5
                    readonly property bool onSide: grip.modelData.fy === 0.5
                    // A side's handle is a short bar along it, as long as the side has room for.
                    readonly property real along: Math.max(8, Math.min(28, (grip.onSide ? selection.height : selection.width) - 28))
                    readonly property bool active: controller.handle === grip.modelData.name
                    // A side too short for a bar beside its corners has only the corners.
                    visible: grip.corner || selection.handleSet !== "all" || (grip.onSide ? selection.height : selection.width) >= 56
                    width: grip.corner ? 14 : grip.onSide ? 6 : grip.along
                    height: grip.corner ? 14 : grip.onSide ? grip.along : 6
                    x: Math.round(grip.modelData.fx * (selection.width - 2) + 1 - grip.width / 2)
                    y: Math.round(grip.modelData.fy * (selection.height - 2) + 1 - grip.height / 2)
                    radius: Math.min(grip.width, grip.height) / 2
                    color: grip.active ? Appearance.colors.colPrimary : Appearance.colors.colOnPrimary
                    border.width: 2
                    border.color: Appearance.colors.colPrimary
                    scale: grip.active ? 1.35 : 1
                    Behavior on scale {
                        NumberAnimation {
                            duration: 100
                            easing.type: Easing.OutCubic
                        }
                    }
                    Behavior on color {
                        ColorAnimation {
                            duration: 100
                        }
                    }
                }
            }
        }

        Repeater {
            model: controller.guides

            delegate: Rectangle {
                id: guide
                required property var modelData
                readonly property bool vertical: guide.modelData.x1 === guide.modelData.x2
                x: guide.vertical ? guide.modelData.x1 - 0.5 : guide.modelData.x1
                y: guide.vertical ? guide.modelData.y1 : guide.modelData.y1 - 0.5
                width: guide.vertical ? 1 : guide.modelData.x2 - guide.modelData.x1
                height: guide.vertical ? guide.modelData.y2 - guide.modelData.y1 : 1
                color: Appearance.colors.colTertiary
            }
        }

        Rectangle {
            visible: controller.marker !== null
            x: controller.marker?.x ?? 0
            y: controller.marker?.y ?? 0
            width: controller.marker?.width ?? 0
            height: controller.marker?.height ?? 0
            radius: 2
            color: Appearance.colors.colPrimary
        }

        // The drag ghost. A drop that would do nothing reads as refused by shape and contrast, not
        // by hue: a faint neutral box with a thin outline, the "not allowed" pointer and badge,
        // where an accepted one is a filled box with a strong accent outline.
        Rectangle {
            id: ghost
            readonly property bool valid: ghost.rect?.valid ?? false
            // A desktop widget that is carried is itself where the pointer has it: its landing
            // place shows only where it is somewhere else (snapped to a neighbour, fitted off one),
            // and moves there softly instead of jumping from place to place.
            readonly property bool carried: controller.ghost?.carried ?? false
            readonly property bool lands: controller.ghost?.valid ?? false
            readonly property bool wanted: controller.ghost !== null && !(selection.resizing && ghost.lands) && (!ghost.carried || !ghost.lands || (controller.ghost.apart ?? true))
            property var rect: null
            readonly property var now: ghost.wanted ? controller.ghost : null
            onNowChanged: {
                if (ghost.now === null)
                    return;
                // Where it was not shown it starts in place; then it slides.
                ghost.eased = ghost.carried && ghost.opacity > 0;
                ghost.rect = ghost.now;
            }
            property bool eased: false
            // A resize that can be done shows as the selection's own outline taking the new size.
            opacity: ghost.wanted ? 1 : 0
            visible: ghost.opacity > 0
            x: ghost.rect?.x ?? 0
            y: ghost.rect?.y ?? 0
            width: ghost.rect?.width ?? 0
            height: ghost.rect?.height ?? 0
            Behavior on opacity {
                NumberAnimation {
                    duration: 100
                    easing.type: Easing.OutCubic
                }
            }
            Behavior on x {
                enabled: ghost.eased
                NumberAnimation {
                    duration: 120
                    easing.type: Easing.OutCubic
                }
            }
            Behavior on y {
                enabled: ghost.eased
                NumberAnimation {
                    duration: 120
                    easing.type: Easing.OutCubic
                }
            }
            radius: Appearance.rounding.small
            color: ghost.valid ? ColorUtils.transparentize(Appearance.colors.colPrimary, 0.7) : ColorUtils.transparentize(Appearance.colors.colOnSurface, 0.9)
            border.width: ghost.valid ? 2 : 1
            border.color: ghost.valid ? Appearance.colors.colPrimary : ColorUtils.transparentize(Appearance.colors.colOutline, 0.3)
        }

        // What a resize gives, under the outline it pulls (above it at the screen's bottom): the
        // size, and in the accent colour with a word when the widget cannot get smaller or larger.
        Rectangle {
            id: sizeBadge
            property var hint: null
            readonly property var now: controller.sizeHint
            onNowChanged: {
                if (sizeBadge.now !== null)
                    sizeBadge.hint = sizeBadge.now;
            }
            readonly property bool limit: (sizeBadge.hint?.smallest ?? false) || (sizeBadge.hint?.largest ?? false)
            readonly property string size: {
                const h = sizeBadge.hint;
                if (h === null)
                    return "";
                if (h.kind === "sidebar")
                    return Translation.tr("%1 of %2 columns").arg(h.width).arg(h.height);
                if (h.kind === "panel")
                    return h.width + " px";
                return h.width + " × " + h.height;
            }
            readonly property real below: selection.y + selection.height + 8
            x: Math.round(Math.max(4, Math.min(root.width - sizeBadge.width - 4, selection.x + (selection.width - sizeBadge.width) / 2)))
            y: Math.round(sizeBadge.below + sizeBadge.height + 4 <= root.height ? sizeBadge.below : selection.y - sizeBadge.height - 8)
            width: sizeText.implicitWidth + 20
            height: sizeText.implicitHeight + 10
            radius: sizeBadge.height / 2
            color: sizeBadge.limit ? Appearance.colors.colPrimary : Appearance.m3colors.m3surfaceContainerHighest
            opacity: sizeBadge.now !== null ? 1 : 0
            visible: sizeBadge.opacity > 0
            Behavior on opacity {
                NumberAnimation {
                    duration: 120
                }
            }
            Behavior on color {
                ColorAnimation {
                    duration: 120
                }
            }

            StyledText {
                id: sizeText
                anchors.centerIn: parent
                text: sizeBadge.size + (!sizeBadge.limit ? "" : " · " + (sizeBadge.hint.smallest ? Translation.tr("smallest") : Translation.tr("largest")))
                color: sizeBadge.limit ? Appearance.colors.colOnPrimary : Appearance.colors.colOnSurface
                font.pixelSize: Appearance.font.pixelSize.small
                font.weight: Font.Medium
            }
        }

        // The refused drop's badge, a fixed size beside the pointer and clear of it (the controller
        // places it), so it reads the same over a thin bar slot as over a large desktop box.
        StyledRectangularShadow {
            target: refusalBadge
            visible: refusalBadge.visible
        }
        Rectangle {
            id: refusalBadge
            visible: controller.refusal !== null
            x: controller.refusal?.x ?? 0
            y: controller.refusal?.y ?? 0
            width: controller.refusal?.width ?? 0
            height: controller.refusal?.height ?? 0
            radius: width / 2
            color: Appearance.m3colors.m3surfaceContainerHighest
            border.width: 1
            border.color: Appearance.colors.colOutline

            MaterialSymbol {
                anchors.centerIn: parent
                text: "block"
                iconSize: Math.round(parent.width * 0.7)
                color: Appearance.colors.colOnSurface
            }
        }

        // What is done to the selected widget, right by it: its settings and its removal. Above
        // its outline's end, under it where there is no room above (a top bar's widget).
        Toolbar {
            id: quick
            readonly property bool shown: controller.selectedFrame !== null && !controller.dragging
            readonly property real above: selection.y - quick.height - 12
            padding: 4
            width: quick.implicitWidth
            height: 40
            enableShadow: quick.shown
            x: Math.round(Math.max(4, Math.min(root.width - quick.width - 4, selection.x + selection.width - quick.width)))
            y: Math.round(quick.above >= 4 ? quick.above : selection.y + selection.height + 12)
            opacity: quick.shown ? 1 : 0
            visible: quick.opacity > 0
            Behavior on opacity {
                NumberAnimation {
                    duration: 120
                    easing.type: Easing.OutCubic
                }
            }

            IconToolbarButton {
                focusPolicy: Qt.NoFocus
                text: "tune"
                onClicked: settingsDialog.open()
                StyledToolTip {
                    text: Translation.tr("Widget settings")
                }
            }
            IconToolbarButton {
                focusPolicy: Qt.NoFocus
                text: "delete"
                onClicked: controller.removeSelected()
                StyledToolTip {
                    text: Translation.tr("Remove (Del)")
                }
            }
        }

        Toolbar {
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Appearance.sizes.elevationMargin * 4 + (LayoutState.panelEdge(root.screen?.name ?? "") === "bottom" ? Appearance.sizes.barHeight : 0)

            IconAndTextToolbarButton {
                focusPolicy: Qt.NoFocus
                iconText: "add"
                text: Translation.tr("Add widget")
                onClicked: catalogDialog.open()
            }
            IconToolbarButton {

                focusPolicy: Qt.NoFocus
                text: "undo"
                enabled: controller.canUndo
                onClicked: controller.undo()
                StyledToolTip {
                    text: Translation.tr("Undo (Ctrl+Z)")
                }
            }
            IconToolbarButton {
                focusPolicy: Qt.NoFocus
                text: "redo"
                enabled: controller.canRedo
                onClicked: controller.redo()
                StyledToolTip {
                    text: Translation.tr("Redo (Ctrl+Shift+Z)")
                }
            }
            IconToolbarButton {
                focusPolicy: Qt.NoFocus
                visible: controller.selectedKind === "panel"
                text: "fit_width"
                rotation: controller.selectedVertical ? 90 : 0
                onClicked: controller.setWidthMode("auto")
                StyledToolTip {
                    text: Translation.tr("Fit content")
                }
            }
            IconToolbarButton {
                focusPolicy: Qt.NoFocus
                visible: controller.selectedKind === "panel"
                text: "expand"
                rotation: controller.selectedVertical ? 0 : 90
                onClicked: controller.setWidthMode("stretch")
                StyledToolTip {
                    text: Translation.tr("Stretch")
                }
            }
            IconToolbarButton {
                focusPolicy: Qt.NoFocus
                text: "side_navigation"
                toggled: GlobalStates.sidebarRightOpen
                onClicked: GlobalStates.sidebarRightOpen = !GlobalStates.sidebarRightOpen
                StyledToolTip {
                    text: Translation.tr("Show the sidebar")
                }
            }
            IconAndTextToolbarButton {
                focusPolicy: Qt.NoFocus
                iconText: "check"
                text: Translation.tr("Done")
                onClicked: controller.finish()
            }
        }

        StyleReviewBar { screenName: root.screen?.name ?? "" }

        // The widget catalog. It is made with the overlay, so that opening it only slides it in
        // (made on demand it would take the frames its opening animation runs for).
        Loader {
            id: catalogDialog
            anchors.fill: parent
            property bool showing: false
            active: true
            function open() {
                // One panel at a time: the settings panel gives way.
                if (settingsDialog.shown)
                    settingsDialog.item.dismiss();
                root.addedToSidebar = "";
                showing = true;
                item.refused = false;
                item.show = true;
                item.forceActiveFocus();
            }
            sourceComponent: WidgetPalette {
                catalog: controller.catalog
                // The widget `type` added to `surface`, its id; "" and `refused` with no room.
                function add(type, surface) {
                    const added = controller.addWidget(type, surface);
                    if (added === "") {
                        refused = true;
                        return "";
                    }
                    // A desktop widget its config flag hides would be added unseen: turn it on.
                    const flag = catalog.find(t => t.type === type)?.enabledBy ?? "";
                    if (surface === "desktop" && flag !== "")
                        Config.setNestedValue(flag, true);
                    return added;
                }
                onPicked: (type, surface) => {
                    const added = add(type, surface);
                    if (added === "")
                        return;
                    if (surface === "sidebar") {
                        GlobalStates.sidebarRightOpen = true;
                        root.addedToSidebar = added;
                    }
                    dismiss();
                }
                // A widget dragged out of the catalog is added and picked up by its middle:
                // from there on it is a drag like any other, let go where the pointer is. It
                // can be picked up once the desktop shows it, a frame or two after it is added.
                property string carried: ""
                property bool held: false
                function pickUp() {
                    const f = controller.frames.find(f => f.id === carried);
                    if (f === undefined)
                        return false;
                    held = controller.press(f.x + f.width / 2, f.y + f.height / 2);
                    return held;
                }
                onCarryStarted: (type, x, y) => {
                    held = false;
                    carried = add(type, "desktop");
                    if (carried === "")
                        carrying = false;
                }
                onCarryMoved: (x, y) => {
                    if (held || pickUp())
                        controller.move(x, y);
                }
                onCarryEnded: (x, y) => {
                    if (x < 0) {
                        controller.cancel();
                    } else if (held || pickUp()) {
                        controller.move(x, y);
                        if (controller.release(x, y))
                            root.placedByHand();
                    }
                    carried = "";
                    held = false;
                    dismiss();
                }
                onDismiss: {
                    show = false;
                    catalogDialog.showing = false;
                    input.forceActiveFocus();
                }
            }
        }

        // The selected widget's settings; made with the overlay like the catalog above.
        Loader {
            id: settingsDialog
            anchors.fill: parent
            property bool shown: false
            active: true
            function open() {
                // One panel at a time: the catalog gives way.
                if (catalogDialog.showing)
                    catalogDialog.item.dismiss();
                shown = true;
                item.open();
                item.forceActiveFocus();
            }
            sourceComponent: WidgetSettingsDialog {
                connector: controller.connector
                widgetId: controller.selectedId
                kind: controller.selectedKind
                place: controller.selectedPlace
                avoid: controller.selectedFrame
                others: controller.frames
                busy: controller.dragging
                onZonePicked: zone => refused = !controller.setZone(zone)
                onWidthPicked: mode => refused = !controller.setWidthMode(mode)
                onSpanPicked: size => refused = !controller.setSpan(size)
                onSizePicked: (width, height) => refused = !controller.setSize(width, height)
                onDismiss: {
                    show = false;
                    settingsDialog.shown = false;
                    input.forceActiveFocus();
                }
            }
        }
    }
}
