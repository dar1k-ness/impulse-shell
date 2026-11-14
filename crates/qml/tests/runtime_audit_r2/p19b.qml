import QtQuick
import QtQuick.Controls
Item {
    ProgressBar { id: m; LayoutMirroring.enabled: true; value: 0.25
        onMirroredChanged: console.log("m mirroredChanged", mirrored)
        onVisualPositionChanged: console.log("m visualPositionChanged", visualPosition)
    }
    Component.onCompleted: console.log("m", m.position, m.visualPosition, m.mirrored)
}
