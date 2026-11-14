import QtQuick
import Rustshell
import ri.modules.common
import ri.modules.rusty.overlay

StyledOverlayWidget {
    id: root
    title: "MangoHud FPS"
    minimumWidth: 275
    minimumHeight: 100
    contentItem: FpsLimiterContent {
        radius: root.contentRadius
    }
}
