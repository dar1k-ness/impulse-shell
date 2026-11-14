import QtQuick
import ri.modules.common

/**
 * the original's background weather widget as a desktop-container widget: the container places it, at
 * the widget's own size as the original draws it; a box resized in edit mode has the widget fill it
 * scaled to fit. When the original's weather placement setting is "leastBusy" or "mostBusy" it goes
 * where the original puts it, by the wallpaper, at its own size. With "free" ("Draggable" in the
 * settings) the pointer drags it, as in the original, and the layout keeps where it was dropped.
 */
Item {
    id: root

    // From the container (WidgetHost.placement).
    property real screenWidth: 0
    property real screenHeight: 0
    property real screenX: 0
    property real screenY: 0
    // The DesktopContainer and this widget's id in it: a widget placed by the wallpaper shows
    // where the container fits it (on the monitor, off the other widgets).
    property var container: null
    property string widgetId: ""
    // A widget of an unplugged monitor, shown here until that monitor comes back.
    property bool guest: false
    // the original's setting, as for the original's own background weather: Settings > Background changes it live.
    readonly property string placementStrategy: Config.options.background.widgets.weather.placementStrategy
    readonly property bool wallpaperPlaced: root.placementStrategy !== "free"
    // the original's "free" placement: the pointer drags the widget.
    readonly property bool movable: !root.wallpaperPlaced && !root.guest
    // Where the widget rests in this item; a drag moves it away from there until it is dropped.
    readonly property real restX: root.wallpaperPlaced ? root.fitted.x - root.screenX : (root.width - weather.width) / 2
    readonly property real restY: root.wallpaperPlaced ? root.fitted.y - root.screenY : (root.height - weather.height) / 2
    implicitWidth: weather.implicitWidth
    implicitHeight: weather.implicitHeight
    // Where the widget draws on the monitor when it places itself (null when the layout does):
    // the container's cell, and so its edit-mode frame, follows it.
    readonly property var placedAt: root.wallpaperPlaced ? root.fitted : null
    readonly property var fitted: {
        const c = root.container;
        const want = { "x": weather.targetX, "y": weather.targetY };
        if (!root.wallpaperPlaced || !c || !c.shown || c.width <= 0 || c.height <= 0)
            return want;
        return c.fitWidget(root.widgetId, want.x, want.y, weather.width, weather.height);
    }

    // The drag ended: the layout gets the place the widget was dropped at (and the original's options, as
    // the original's own widget writes them), and the widget rests in this item again, which the
    // container shows there.
    function dropped() {
        const dx = weather.x - root.restX;
        const dy = weather.y - root.restY;
        weather.x = Qt.binding(function () {
            return root.restX;
        });
        weather.y = Qt.binding(function () {
            return root.restY;
        });
        if (!root.movable || !root.container || (dx === 0 && dy === 0))
            return;
        const x = Math.round(root.screenX + dx);
        const y = Math.round(root.screenY + dy);
        if (root.container.moveWidget(root.widgetId, x, y)) {
            Config.options.background.widgets.weather.x = x;
            Config.options.background.widgets.weather.y = y;
        }
    }

    WeatherWidget {
        id: weather
        screenWidth: root.screenWidth
        screenHeight: root.screenHeight
        scaledScreenWidth: root.screenWidth
        scaledScreenHeight: root.screenHeight
        wallpaperScale: 1
        placementStrategy: root.placementStrategy
        draggable: root.movable
        animateXPos: false
        animateYPos: false
        savesPlace: false
        x: root.restX
        y: root.restY
        scale: (root.wallpaperPlaced ? 1 : Math.min(root.width / Math.max(1, weather.implicitWidth), root.height / Math.max(1, weather.implicitHeight))) * (weather.draggable && weather.containsPress ? 1.05 : 1)
        onReleased: root.dropped()
    }
}
