import QtQuick
import Rustshell
import Impulse
import ri.modules.common

/**
 * The desktop container of the widget layout (rusty-impulse only: Background.qml loads this file
 * only when the layout has a desktop container, so `qs -c rusty-impulse` never parses it). Widgets sit
 * where the layout puts them, fitted on the monitor and off each other.
 */
DesktopContainer {
    id: root

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
            // A widget the layout gives no size has its own, as QML sizes an item by its
            // implicit size; the container fits it on the monitor and off the others by it.
            readonly property bool natural: cell.modelData.natural ?? false
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

            x: cell.box.x
            y: cell.box.y
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
            }
            onSelfPlacedChanged: root.setSelfPlaced(cell.key, cell.selfPlaced)
            onShowingChanged: root.setHidden(cell.key, !cell.showing)
            Component.onDestruction: {
                root.setSelfPlaced(cell.key, false);
                root.setHidden(cell.key, false);
            }

            WidgetHost {
                id: host
                anchors.fill: parent
                widgetType: cell.showing ? cell.modelData.type : ""
                settings: cell.modelData.settings
                placement: ({
                        screenWidth: root.width,
                        screenHeight: root.height,
                        screenX: cell.x,
                        screenY: cell.y,
                        container: root,
                        widgetId: cell.key,
                        guest: (cell.modelData.guest ?? "") !== "",
                        wallpaperSafetyTriggered: root.wallpaperSafetyTriggered
                    })
            }
        }
    }
}
