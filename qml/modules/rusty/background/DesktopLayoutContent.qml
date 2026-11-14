import QtQuick
import Rustshell
import Impulse
import ri.modules.common

/**
 * The desktop container of the edit-mode layout (rusty-impulse only: Background.qml loads this file
 * only when the layout has a desktop container, so `qs -c rusty-impulse` never parses it). Widgets sit
 * where the layout puts them; while `editing`, they can be dragged and resized from the
 * corner handle, and the container answers with the nearest place that fits (no overlaps,
 * on the monitor, on the grid when `gridSize` is set). A right-click on the desktop opens
 * its menu (DesktopMenu.qml).
 */
DesktopContainer {
    id: root

    property bool editing: false
    // The background window (Background.qml's bgRoot) this content lives in.
    readonly property var window: root.QsWindow.window
    readonly property bool wallpaperSafetyTriggered: root.window?.wallpaperSafetyTriggered ?? false

    connector: root.window?.screen?.name ?? ""

    // Whether config flag `path` (widgets.toml `enabledBy`, a dotted path into
    // Config.options) is on: anything but `false`. No flag: always on.
    function flagOn(path: string): bool {
        if (path === "")
            return true;
        const keys = path.split(".");
        let v = Config.options;
        for (let i = 0; i < keys.length && v !== undefined && v !== null; i++)
            v = v[keys[i]];
        return v !== false;
    }

    // As on Plasma's desktop, a right-click on it opens the desktop menu; its "Enter edit mode"
    // starts edit mode in this process (edit mode is one state for the whole shell).
    EditController {
        id: editController
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.RightButton
        enabled: !editController.active
        onPressed: mouse => {
            desktopMenu.position = Qt.point(mouse.x, mouse.y);
            desktopMenu.active = true;
        }
    }

    Loader {
        id: desktopMenu
        property point position
        active: false
        sourceComponent: DesktopMenu {
            position: desktopMenu.position
            anchor.window: root.window
            onEditRequested: editController.start()
            onClosed: desktopMenu.active = false
        }
    }

    Repeater {
        model: root.widgets

        delegate: Item {
            id: cell
            required property var modelData
            // Where it shows on this monitor (fitted after a resolution or scale change).
            readonly property var box: root.shown[cell.key] ?? cell.modelData
            // What names it in `shown` and `setNaturalSize`: its id, or for a widget of an
            // unplugged monitor shown here until that monitor comes back, a key of its own.
            readonly property string key: cell.modelData.key ?? cell.modelData.id
            // A widget of an unplugged monitor is edited on its own monitor, not here.
            readonly property bool visiting: (cell.modelData.guest ?? "") !== ""
            readonly property bool editable: root.editing && !cell.visiting
            // A widget the layout gives no size has its own, as QML sizes an item by its
            // implicit size; the container fits it on the monitor and off the others by it.
            // A resize in edit mode shows the new size live (`box.sized`) before it is saved.
            readonly property bool natural: (cell.modelData.natural ?? false) && !(cell.box.sized ?? false)
            readonly property real naturalWidth: host.item?.implicitWidth ?? 0
            readonly property real naturalHeight: host.item?.implicitHeight ?? 0
            // A widget that places itself (the original's wallpaper placement) shows where it puts
            // itself, off the others (fitWidget): the others do not keep off its layout place.
            readonly property bool selfPlaced: host.item?.wallpaperPlaced ?? false
            // A widget shows while the config flag its type is enabled by is on, fading in and
            // out as the original's FadeLoader does; faded out, it is not loaded and takes no place.
            readonly property bool turnedOff: !root.flagOn(cell.modelData.enabledBy ?? "")
            readonly property bool showing: cell.opacity > 0

            function reportNaturalSize() {
                if (cell.natural && cell.naturalWidth > 0 && cell.naturalHeight > 0)
                    root.setNaturalSize(cell.key, cell.naturalWidth, cell.naturalHeight);
            }

            // A widget that places itself (by the wallpaper) has its cell where it draws, and
            // the container fits the other widgets off it there, not off its layout place.
            readonly property var placedAt: host.item?.placedAt ?? null

            function reportPlacement() {
                const p = cell.placedAt;
                if (p)
                    root.setPlacedAt(cell.key, p.x, p.y, p.width ?? cell.width, p.height ?? cell.height);
                else
                    root.clearPlacedAt(cell.key);
            }

            onPlacedAtChanged: cell.reportPlacement()
            x: cell.placedAt ? cell.placedAt.x : cell.box.x
            y: cell.placedAt ? cell.placedAt.y : cell.box.y
            width: cell.natural && cell.naturalWidth > 0 ? cell.naturalWidth : cell.box.width
            height: cell.natural && cell.naturalHeight > 0 ? cell.naturalHeight : cell.box.height

            opacity: cell.turnedOff ? 0 : 1
            visible: cell.showing
            Behavior on opacity {
                animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
            }

            onNaturalWidthChanged: cell.reportNaturalSize()
            onNaturalHeightChanged: cell.reportNaturalSize()
            Component.onCompleted: {
                root.setHidden(cell.key, !cell.showing);
                cell.reportNaturalSize();
                cell.reportPlacement();
            }
            onSelfPlacedChanged: root.setSelfPlaced(cell.key, cell.selfPlaced)
            onShowingChanged: root.setHidden(cell.key, !cell.showing)
            Component.onDestruction: {
                root.clearPlacedAt(cell.key);
                root.setSelfPlaced(cell.key, false);
                root.setHidden(cell.key, false);
            }

            WidgetHost {
                id: host
                anchors.fill: parent
                surface: "desktop"
                widgetType: cell.showing ? cell.modelData.type : ""
                // The edit overlay finds a widget by it on this monitor's layout.
                layoutId: cell.visiting ? "" : cell.modelData.id
                settings: cell.modelData.settings
                placement: ({
                        screenWidth: root.width,
                        screenHeight: root.height,
                        screenX: cell.x,
                        screenY: cell.y,
                        container: root,
                        widgetId: cell.key,
                        // The box has a size of the layout's, not the widget's own.
                        sized: !cell.natural,
                        guest: (cell.modelData.guest ?? "") !== "",
                        wallpaperSafetyTriggered: root.wallpaperSafetyTriggered
                    })
            }

            Rectangle {
                anchors.fill: parent
                visible: cell.editable
                color: "transparent"
                radius: Appearance.rounding.normal
                border.width: 2
                border.color: Appearance.colors.colPrimary
            }

            MouseArea {
                id: moveArea
                anchors.fill: parent
                enabled: cell.editable
                visible: cell.editable
                drag.target: cell
                cursorShape: moveArea.pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                onReleased: {
                    if (!root.placeWidget(cell.modelData.id, cell.x, cell.y)) {
                        cell.x = cell.box.x;
                        cell.y = cell.box.y;
                    }
                }
            }

            MouseArea {
                id: resizeArea
                property real pressX: 0
                property real pressY: 0
                property real startWidth: 0
                property real startHeight: 0
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                width: 20
                height: 20
                enabled: cell.editable
                visible: cell.editable
                cursorShape: Qt.SizeFDiagCursor
                onPressed: mouse => {
                    resizeArea.pressX = mapToItem(root, mouse.x, mouse.y).x;
                    resizeArea.pressY = mapToItem(root, mouse.x, mouse.y).y;
                    resizeArea.startWidth = cell.width;
                    resizeArea.startHeight = cell.height;
                }
                onPositionChanged: mouse => {
                    const p = mapToItem(root, mouse.x, mouse.y);
                    cell.width = Math.max(1, resizeArea.startWidth + p.x - resizeArea.pressX);
                    cell.height = Math.max(1, resizeArea.startHeight + p.y - resizeArea.pressY);
                }
                onReleased: {
                    if (!root.resizeWidget(cell.modelData.id, cell.width, cell.height)) {
                        cell.width = cell.box.width;
                        cell.height = cell.box.height;
                    }
                }

                Rectangle {
                    anchors.fill: parent
                    anchors.margins: 4
                    radius: width / 2
                    color: Appearance.colors.colPrimary
                }
            }
        }
    }
}
