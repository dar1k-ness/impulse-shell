import QtQuick
import ri.modules.common

/**
 * the original's background clock as a desktop-container widget: the container places it (the box is
 * the layout's x/y), at the clock's own size as the original draws it; a box resized in edit mode has
 * the clock fill it scaled to fit. On the lock screen with `lock.centerClock` it still
 * centers on the monitor. When the original's clock placement setting is "leastBusy" or "mostBusy" the
 * clock goes where the original puts it, by the wallpaper, at its own size. With "free" ("Draggable" in
 * the settings) the pointer drags it, as in the original, and the layout keeps where it was dropped.
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
    property bool wallpaperSafetyTriggered: false
    // the original's setting, as for the original's own background clock: Settings > Background changes it live.
    readonly property string placementStrategy: Config.options.background.widgets.clock.placementStrategy
    readonly property bool wallpaperPlaced: root.placementStrategy !== "free"
    // the original's "free" placement: the pointer drags the widget.
    readonly property bool movable: !root.wallpaperPlaced && !root.guest && !clock.forceCenter
    // Where the widget rests in this item; a drag moves it away from there until it is dropped.
    readonly property real restX: clock.forceCenter ? (root.screenWidth - clock.width) / 2 - root.screenX : root.wallpaperPlaced ? root.fitted.x - root.screenX : (root.width - clock.width) / 2
    readonly property real restY: clock.forceCenter ? (root.screenHeight - clock.height) / 2 - root.screenY : root.wallpaperPlaced ? root.fitted.y - root.screenY : (root.height - clock.height) / 2
    implicitWidth: clock.implicitWidth
    implicitHeight: clock.implicitHeight
    // Where the clock draws on the monitor when it places itself (null when the layout does):
    // the container's cell, and so its edit-mode frame, follows it.
    readonly property var placedAt: clock.forceCenter ? { "x": (root.screenWidth - clock.width) / 2, "y": (root.screenHeight - clock.height) / 2 } : root.wallpaperPlaced ? root.fitted : null
    readonly property var fitted: {
        const c = root.container;
        const want = { "x": clock.targetX, "y": clock.targetY };
        if (!root.wallpaperPlaced || !c || !c.shown || c.width <= 0 || c.height <= 0)
            return want;
        // What paints is kept on the monitor: an empty status row may hang over the edge.
        return c.fitWidget(root.widgetId, want.x, want.y, clock.width, Math.max(1, clock.height - clock.unpaintedBottom));
    }

    // The drag ended: the layout gets the place the widget was dropped at (and the original's options, as
    // the original's own widget writes them), and the widget rests in this item again, which the
    // container shows there.
    function dropped() {
        const dx = clock.x - root.restX;
        const dy = clock.y - root.restY;
        clock.x = Qt.binding(function () {
            return root.restX;
        });
        clock.y = Qt.binding(function () {
            return root.restY;
        });
        if (!root.movable || !root.container || (dx === 0 && dy === 0))
            return;
        const x = Math.round(root.screenX + dx);
        const y = Math.round(root.screenY + dy);
        if (root.container.moveWidget(root.widgetId, x, y)) {
            Config.options.background.widgets.clock.x = x;
            Config.options.background.widgets.clock.y = y;
        }
    }

    ClockWidget {
        id: clock
        screenWidth: root.screenWidth
        screenHeight: root.screenHeight
        scaledScreenWidth: root.screenWidth
        scaledScreenHeight: root.screenHeight
        wallpaperScale: 1
        wallpaperSafetyTriggered: root.wallpaperSafetyTriggered
        placementStrategy: root.placementStrategy
        draggable: root.movable
        animateXPos: false
        animateYPos: false
        savesPlace: false
        x: root.restX
        y: root.restY
        scale: (clock.forceCenter || root.wallpaperPlaced ? 1 : Math.min(root.width / Math.max(1, clock.implicitWidth), root.height / Math.max(1, clock.implicitHeight))) * clock.lift
        onReleased: root.dropped()
        textHorizontalAlignment: {
            if (!Config.options.background.widgets.clock.digital.adaptiveAlignment || clock.forceCenter || Config.options.background.widgets.clock.digital.vertical)
                return Text.AlignHCenter;
            if (root.screenX < root.screenWidth / 3)
                return Text.AlignLeft;
            if (root.screenX > root.screenWidth * 2 / 3)
                return Text.AlignRight;
            return Text.AlignHCenter;
        }
    }
}
