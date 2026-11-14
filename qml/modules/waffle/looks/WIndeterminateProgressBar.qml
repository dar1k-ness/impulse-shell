import QtQuick
import Qt5Compat.GraphicalEffects
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.waffle.looks

StyledIndeterminateProgressBar {
    id: progressBar
    implicitHeight: 3
    background: null
    layer.enabled: true
    layer.effect: OpacityMask {
        maskSource: Rectangle {
            width: progressBar.width
            height: progressBar.height
            radius: progressBar.height / 2
        }
    }
}
