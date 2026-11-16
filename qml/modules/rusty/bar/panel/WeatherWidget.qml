import QtQuick
import ri.modules.common
import ri.modules.rusty.bar as Bar
import ri.modules.rusty.bar.weather

Loader {
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    property real fixedLength: parent?.fixedLength ?? -1
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    // Stretching or pulled by an end, it gives way no further than what it shows takes.
    readonly property real leastLength: vertical ? implicitHeight : implicitWidth
    active: !vertical && Config.options.bar.weather.enable
    readonly property bool shown: active
    visible: shown
    sourceComponent: Bar.BarGroup {
        WeatherBar {
            room: fixedLength
        }
    }
}
