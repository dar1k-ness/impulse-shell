import QtQuick

// Pointer handlers inside transformed items, the way the shell's own QML places them: a
// DragHandler that moves its target (StyledOverlayWidget), one with no target that reports
// its travel (TaskViewContent, WPolkitContent), and a WheelHandler (FloatingImage,
// SecondaryTabBar). tst_transform_handlers.qml runs this file in Qt 6.12,
// ../transform_live.rs in the engine: both make the same moves and expect the same values.
Rectangle {
    id: root
    width: 300; height: 200
    color: "black"
    property string log: ""
    property alias card: card
    property alias small: small
    property alias widget: widget
    function say(s) { log += s + " " }
    function r(v) { return Math.round(v * 10) / 10 }

    // A DragHandler that moves its target inside a turned item: the item shows at
    // 20..40 x 20..120, the card at its top; down the window is along the item's x, right in
    // the window is against its y.
    Item {
        x: 40; y: 20; width: 100; height: 20
        transform: Rotation { angle: 90 }
        Rectangle {
            id: card
            width: 20; height: 20; color: "#ffffff"
            DragHandler {
                target: card
                xAxis.minimum: 0; xAxis.maximum: 70
                yAxis.minimum: -8; yAxis.maximum: 8
            }
        }
    }
    // A DragHandler with no target inside an item at half size, which shows at
    // 115..145 x 30..50: its activeValue is the travel in the window.
    Rectangle {
        x: 100; y: 20; width: 60; height: 40; color: "#3366ff"
        scale: 0.5
        DragHandler {
            target: null
            xAxis.onActiveValueChanged: root.say("x " + root.r(xAxis.activeValue))
            yAxis.onActiveValueChanged: root.say("y " + root.r(yAxis.activeValue))
            onActiveChanged: root.say("free " + active)
        }
    }
    // A WheelHandler in a turned item at twice its width: the item shows at
    // 230..260 x 60..180, and not at 260..300 x 60..90 where it is laid out.
    Rectangle {
        x: 260; y: 60; width: 60; height: 30; color: "#ffcc00"
        transform: [ Scale { xScale: 2 }, Rotation { angle: 90 } ]
        WheelHandler {
            acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
            onWheel: event => root.say("wheel " + event.angleDelta.y + " at " + root.r(event.x) + "," + root.r(event.y))
        }
    }
    // A DragHandler whose target is itself at half size about its centre: it shows at
    // 215..245 x 15..35 and follows the pointer one for one.
    Rectangle {
        id: small
        x: 200; y: 5; width: 60; height: 40; color: "#ff3366"
        scale: 0.5
        DragHandler { target: small }
    }
    // An overlay widget while the overlay opens (OverlayContent grows from 1.08 to 1): the pane
    // shows at 54..216 x 127.6..192.4, the widget is kept inside it and takes the wheel.
    Item {
        id: pane
        x: 60; y: 130; width: 150; height: 60
        scale: 1.08
        Rectangle {
            id: widget
            x: 10; y: 10; width: 40; height: 20; color: "#33cc66"
            DragHandler {
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                target: widget
                xAxis.minimum: 0; xAxis.maximum: pane.width - widget.width
                yAxis.minimum: 0; yAxis.maximum: pane.height - widget.height
            }
            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: event => root.say("zoom " + event.angleDelta.y)
            }
        }
    }
}
