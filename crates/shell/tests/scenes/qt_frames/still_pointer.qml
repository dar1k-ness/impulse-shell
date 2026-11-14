// The pointer stands at (50, 50) and the items move by an animation: `area` (a MouseArea)
// slides out from under it from frame 2 and back under it from frame 38, `button` (a Button)
// slides in under it from frame 14 and out from frame 26. Each frame logs the two x and what
// changed since the frame before: Qt delivers hover to a window in its frame, when an item of
// it changed (QQuickDeliveryAgentPrivate::flushFrameSynchronousEvents).
import QtQuick
import QtQuick.Controls
Item {
    id: scene
    width: 200; height: 200
    property int frames: 50
    property string log: ""
    property real mouseX: 50
    property real mouseY: 50
    property string said: ""
    MouseArea {
        id: area
        width: 100; height: 100
        hoverEnabled: true
        onContainsMouseChanged: said += " containsMouse=" + containsMouse
        NumberAnimation { id: slideOut; target: area; property: "x"; to: 150; duration: 160 }
        NumberAnimation { id: slideIn; target: area; property: "x"; to: 0; duration: 160 }
    }
    Button {
        id: button
        x: -150; width: 100; height: 100
        hoverEnabled: true
        onHoveredChanged: said += " hovered=" + hovered
        NumberAnimation { id: glideIn; target: button; property: "x"; to: 0; duration: 160 }
        NumberAnimation { id: glideOut; target: button; property: "x"; to: -150; duration: 160 }
    }
    function sample(frame) {
        if (frame === 2)
            slideOut.start()
        if (frame === 14)
            glideIn.start()
        if (frame === 26)
            glideOut.start()
        if (frame === 38)
            slideIn.start()
        log += frame + ": " + Math.round(area.x) + " " + Math.round(button.x) + said + "\n"
        said = ""
    }
}
