import QtQuick
import ri.modules.common
import ri.modules.common.widgets

/**
 * Bar position choice drawn over the current wallpaper.
 */
SettingsPreviewTile {
    id: root
    required property int position // bit 0: bottom, bit 1: vertical
    readonly property bool vertical: (position & 2) !== 0
    readonly property bool atBottom: (position & 1) !== 0

    tileWidth: 100
    tileHeight: 58
    selected: ((Config.options.bar.bottom ? 1 : 0) | (Config.options.bar.vertical ? 2 : 0)) === root.position
    onClicked: {
        Config.options.bar.bottom = root.atBottom;
        Config.options.bar.vertical = root.vertical;
    }

    SettingsWallpaperThumb {
        radius: Appearance.rounding.small
    }
    Rectangle {
        x: root.vertical ? (root.atBottom ? parent.width - 10 : 4) : 6
        y: root.vertical ? 5 : (root.atBottom ? parent.height - 10 : 4)
        width: root.vertical ? 6 : parent.width - 12
        height: root.vertical ? parent.height - 10 : 6
        radius: 3
        color: "#1d1f24"
        opacity: 0.85
    }
}
