import QtQuick
import ri.modules.common
import ri.modules.rusty.sidebarRight

// Sidebar widget "sliders": the original's brightness, volume and microphone sliders, shown as the original's
// settings say (`sidebar.quickSliders`: Settings > Interface > Sliders), live. With the
// sliders off, or none of the three chosen, the widget is not shown and takes no place.
Item {
    id: root

    readonly property var options: Config.options.sidebar.quickSliders
    readonly property bool shown: root.options.enable && (root.options.showBrightness || root.options.showVolume || root.options.showMic)

    implicitWidth: sliders.item?.implicitWidth ?? 0
    implicitHeight: sliders.item?.implicitHeight ?? 0

    Loader {
        id: sliders
        anchors.fill: parent
        active: root.shown
        sourceComponent: QuickSliders {}
    }
}
