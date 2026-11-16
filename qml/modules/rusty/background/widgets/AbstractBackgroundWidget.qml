import QtQuick
import Rustshell
import Rustshell.Io
import Impulse
import ri
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.utils
import ri.modules.common.widgets.widgetCanvas

AbstractWidget {
    id: root

    required property string configEntryName
    required property int screenWidth
    required property int screenHeight
    required property int scaledScreenWidth
    required property int scaledScreenHeight
    required property real wallpaperScale
    property bool visibleWhenLocked: false
    property var configEntry: Config.options.background.widgets[configEntryName]
    property string placementStrategy: configEntry.placementStrategy
    property real targetX: Math.max(0, Math.min(configEntry.x, scaledScreenWidth - width))
    property real targetY : Math.max(0, Math.min(configEntry.y, scaledScreenHeight - height))
    x: targetX
    y: targetY
    visible: opacity > 0
    opacity: (GlobalStates.screenLocked && !visibleWhenLocked) ? 0 : 1
    Behavior on opacity {
        animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
    }
    // The widget grows a little under the press that drags it, softly. Only that is animated:
    // a scale its container gives it besides (a desktop widget fitted to its box) is taken at
    // once, so the widget stays in its box while the box is resized.
    property real lift: (draggable && containsPress) ? 1.05 : 1
    Behavior on lift {
        animation: Appearance.animation.elementResize.numberAnimation.createObject(this)
    }
    scale: lift

    draggable: placementStrategy === "free"
    // Where a drag leaves the widget is saved in the original's options. A widget the desktop layout
    // places (DesktopClock, DesktopWeather) is saved by its container instead.
    property bool savesPlace: true
    onReleased: {
        if (!root.savesPlace)
            return;
        root.targetX = root.x;
        root.targetY = root.y;
        configEntry.x = root.targetX;
        configEntry.y = root.targetY;
    }

    property bool needsColText: false
    property color dominantColor: Appearance.colors.colPrimary
    property bool dominantColorIsDark: dominantColor.hslLightness < 0.5
    property color colText: {
        const onNormalBackground = (GlobalStates.screenLocked && Config.options.lock.blur.enable)
        const adaptiveColor = ColorUtils.colorWithLightness(Appearance.colors.colPrimary, (dominantColorIsDark ? 0.8 : 0.12))
        return onNormalBackground ? Appearance.colors.colOnLayer0 : adaptiveColor;
    }

    property bool wallpaperIsVideo: Config.options.background.wallpaperPath.endsWith(".mp4") || Config.options.background.wallpaperPath.endsWith(".webm") || Config.options.background.wallpaperPath.endsWith(".mkv") || Config.options.background.wallpaperPath.endsWith(".avi") || Config.options.background.wallpaperPath.endsWith(".mov")
    property string wallpaperPath: wallpaperIsVideo ? Config.options.background.thumbnailPath : Config.options.background.wallpaperPath
    
    onWallpaperPathChanged: refreshPlacementIfNeeded()
    onPlacementStrategyChanged: refreshPlacementIfNeeded()
    // Placed once, as the original does: a placement asked for before the screen is measured runs when
    // its size becomes known; a later resolution or scale change keeps the place (the
    // container keeps it on the monitor).
    property bool placementPending: false
    onScaledScreenWidthChanged: if (root.placementPending) refreshPlacementIfNeeded()
    onScaledScreenHeightChanged: if (root.placementPending) refreshPlacementIfNeeded()
    Component.onCompleted: refreshPlacementIfNeeded()
    Connections {
        target: Config
        function onReadyChanged() { refreshPlacementIfNeeded() }
    }
    function refreshPlacementIfNeeded() {
        if (!Config.ready) return;
        if (root.placementStrategy === "free" && !root.needsColText) return;
        root.placementPending = root.scaledScreenWidth <= 0 || root.scaledScreenHeight <= 0;
        if (root.placementPending) return;
        // Qt runs the handlers that call this before the bindings that read the same
        // properties: the search would start with the previous screen size and strategy.
        leastBusyRegionProc.imagePath = root.wallpaperPath;
        leastBusyRegionProc.screenWidth = Math.round(root.scaledScreenWidth);
        leastBusyRegionProc.screenHeight = Math.round(root.scaledScreenHeight);
        leastBusyRegionProc.busiest = root.placementStrategy === "mostBusy";
        leastBusyRegionProc.running = false;
        leastBusyRegionProc.running = true;
    }
    LeastBusyRegion {
        id: leastBusyRegionProc
        imagePath: root.wallpaperPath
        // TODO: make these less arbitrary
        regionWidth: 300
        regionHeight: 300
        horizontalPadding: 200
        verticalPadding: 200
        screenWidth: Math.round(root.scaledScreenWidth)
        screenHeight: Math.round(root.scaledScreenHeight)
        busiest: root.placementStrategy === "mostBusy"
        onFound: region => {
            root.dominantColor = region.dominant_color || Appearance.colors.colPrimary;
            if (root.placementStrategy === "free") return;
            root.targetX = region.center_x * root.wallpaperScale - root.width / 2;
            root.targetY  = region.center_y * root.wallpaperScale - root.height / 2;
        }
    }
}

