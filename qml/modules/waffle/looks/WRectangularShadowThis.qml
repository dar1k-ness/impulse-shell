import QtQuick
import QtQuick.Effects
import ri.modules.common
import ri.modules.common.widgets

Item {
    default property Item contentItem
    property Item shadow: WRectangularShadow {
        target: contentItem
    }
    implicitWidth: contentItem.implicitWidth
    implicitHeight: contentItem.implicitHeight

    children: [shadow, contentItem]
}
