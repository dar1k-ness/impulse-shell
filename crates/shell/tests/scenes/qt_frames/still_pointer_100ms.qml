// still_pointer.qml at the real interval of frame-synchronous hover, 100 ms of wall time
// (QQuickDeliveryAgentPrivate::frameSynchronousHoverInterval). The pointer stands at (50, 50);
// `wait(frame)` is the wall time that passes after each frame. A frame inside the interval
// delivers no hover and leaves it to the delay timer, which fires between two frames: what
// it changed is logged as `before` the next frame. A frame after the interval delivers it.
//
//   frame   1   2   3  timer   4    5  timer   6  timer   7    8    9  timer  10
//   at ms   0  30  60   100  130  160   200  250   300  450  480  510   550  580
//
// Qt's delay timer is a coarse one and fires up to 8 ms late, each time later than the last,
// so the frames keep 20 ms and more away from it and frame 7 starts the interval anew.
//
// `area` slides out from frame 3, `button` glides in at frames 5 and 6 and out from frame 7,
// `area` slides back at frames 8 and 9.
import QtQuick
import QtQuick.Controls
Item {
    id: scene
    width: 200; height: 200
    property int frames: 10
    property int hoverInterval: 100
    property string log: ""
    property real mouseX: 50
    property real mouseY: 50
    property string said: ""
    MouseArea {
        id: area
        width: 100; height: 100
        hoverEnabled: true
        onContainsMouseChanged: said += " containsMouse=" + containsMouse
        NumberAnimation { id: slideOut; target: area; property: "x"; to: 150; duration: 32 }
        NumberAnimation { id: slideIn; target: area; property: "x"; to: 0; duration: 32 }
    }
    Button {
        id: button
        x: -150; width: 100; height: 100
        hoverEnabled: true
        onHoveredChanged: said += " hovered=" + hovered
        NumberAnimation { id: glideIn; target: button; property: "x"; to: 0; duration: 32 }
        NumberAnimation { id: glideOut; target: button; property: "x"; to: -150; duration: 32 }
    }
    function wait(frame) {
        return [30, 30, 70, 30, 90, 200, 30, 30, 70, 0][frame - 1]
    }
    function sample(frame) {
        if (frame === 2)
            slideOut.start()
        if (frame === 4)
            glideIn.start()
        if (frame === 6)
            glideOut.start()
        if (frame === 7)
            slideIn.start()
        if (said !== "")
            log += "before " + frame + ":" + said + "\n"
        said = ""
    }
    function shown(frame) {
        log += frame + ": " + Math.round(area.x) + " " + Math.round(button.x) + said + "\n"
        said = ""
    }
}
