import QtQuick

// Drags of transformed things: a DragHandler whose own target is turned, axis limits inside a
// scaled item, MouseArea's drag.axis, drag.minimumX / maximumX and drag.threshold inside
// scaled and turned items, and the same drags by a finger. tst_transform_drags.qml runs this
// file in Qt 6.12, ../transform_live.rs in the engine: both make the same moves and expect
// the same values.
Rectangle {
    id: root
    width: 300; height: 200
    color: "black"
    property alias turned: turned
    property alias tilted: tilted
    property alias boxed: boxed
    property alias slider: slider
    property alias puck: puck
    property alias finger: finger
    property alias thumb: thumb
    function r(v) { return Math.round(v * 10) / 10 }

    // A DragHandler whose target is itself a quarter turn about its centre: it shows at
    // 30..50 x 10..50.
    Rectangle {
        id: turned
        x: 20; y: 20; width: 40; height: 20; color: "#ffffff"
        rotation: 90
        DragHandler { target: turned }
    }
    // The same with the turn in the target's transform list, about its corner, and a scale of
    // its own: it shows at 70..100 x 20..80.
    Rectangle {
        id: tilted
        x: 100; y: 20; width: 40; height: 20; color: "#ff3366"
        transform: [ Scale { xScale: 1.5; yScale: 1.5 }, Rotation { angle: 90 } ]
        DragHandler { target: tilted }
    }
    // Axis limits inside an item at twice its size, which shows at 20..140 x 80..140: the
    // limits are in the item's own units, and the y axis is off.
    Item {
        x: 20; y: 80; width: 60; height: 30
        scale: 2; transformOrigin: Item.TopLeft
        Rectangle {
            id: boxed
            width: 10; height: 10; color: "#3366ff"
            DragHandler {
                target: boxed
                xAxis.minimum: 2; xAxis.maximum: 30
                yAxis.enabled: false
            }
        }
    }
    // MouseArea's drag along x with limits and the default threshold, inside an item at twice
    // its size, which shows at 160..280 x 20..60.
    Item {
        x: 160; y: 20; width: 60; height: 20
        scale: 2; transformOrigin: Item.TopLeft
        Rectangle {
            id: slider
            x: 10; width: 10; height: 10; color: "#ffcc00"
            MouseArea {
                anchors.fill: parent
                drag.target: slider; drag.axis: Drag.XAxis
                drag.minimumX: 4; drag.maximumX: 40
            }
        }
    }
    // MouseArea's drag on both axes with a threshold of its own, not smoothed, inside an item
    // upside down at half size, which shows at 215..245 x 110..130.
    Item {
        x: 200; y: 100; width: 60; height: 40
        rotation: 180; scale: 0.5
        Rectangle {
            id: puck
            x: 20; y: 10; width: 20; height: 20; color: "#33cc66"
            MouseArea {
                anchors.fill: parent
                drag.target: puck; drag.threshold: 3; drag.smoothed: false
                drag.minimumY: 0; drag.maximumY: 20
            }
        }
    }
    // The drags of a finger, inside an item at one and a half times its size, which shows at
    // 15..165 x 145..175.
    Item {
        x: 40; y: 150; width: 100; height: 20
        scale: 1.5
        Rectangle {
            id: finger
            width: 20; height: 20; color: "#ffffff"
            DragHandler { target: finger }
        }
        Rectangle {
            id: thumb
            x: 60; width: 20; height: 20; color: "#ff3366"
            MouseArea {
                anchors.fill: parent
                drag.target: thumb
            }
        }
    }
}
