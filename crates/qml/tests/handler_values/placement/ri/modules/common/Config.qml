pragma Singleton
import QtQuick
QtObject {
    property bool ready: true
    property QtObject options: QtObject {
        property QtObject background: QtObject {
            property string wallpaperPath: "/wallpaper.png"
            property string thumbnailPath: ""
            property QtObject widgets: QtObject {
                property QtObject clock: QtObject {
                    property string placementStrategy: "leastBusy"
                    property real x: 0
                    property real y: 0
                }
            }
        }
        property QtObject lock: QtObject {
            property QtObject blur: QtObject { property bool enable: false }
        }
    }
}
