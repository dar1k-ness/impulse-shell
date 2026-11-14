import QtQuick
import Qt5Compat.GraphicalEffects
import ri.modules.common
import ri.modules.common.widgets

/**
 * The current wallpaper, cropped into a rounded thumbnail. Every thumbnail asks for the same
 * decoded size so they share one image.
 */
Item {
    id: root
    property real radius: Appearance.rounding.normal
    anchors.fill: parent

    Rectangle {
        anchors.fill: parent
        radius: root.radius
        color: Appearance.colors.colLayer3
    }
    StyledImage {
        id: image
        anchors.fill: parent
        fillMode: Image.PreserveAspectCrop
        source: Images.wallpaperStillPath
        sourceSize.width: 320
        sourceSize.height: 180
        asynchronous: true
        layer.enabled: true
        layer.effect: OpacityMask {
            maskSource: Rectangle {
                width: image.width
                height: image.height
                radius: root.radius
            }
        }
    }
}
