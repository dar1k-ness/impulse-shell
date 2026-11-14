import QtQuick

// Scene of ../transform_live.rs and of tst_transform_live.qml (the same steps in Qt 6.12):
// transforms that change while the scene is shown (a binding, a Behavior, animations), and
// hover and drags inside transformed items.
Rectangle {
    id: root
    width: 300; height: 200
    color: "black"
    property string log: ""
    property real p: 0
    property alias eased: te
    property alias run: run
    property alias loop: loop
    property alias slide: ts
    property alias knob: knob
    function say(s) { log += s + " " }
    function r(v) { return Math.round(v * 10) / 10 }
    function pt(p) { return r(p.x) + "," + r(p.y) }
    function startRun() { run.start() }
    function startLoop() { loop.start() }
    // Where each moving rectangle's top left corner is in the scene.
    function where() {
        return [bound, easedItem, runItem, loopItem].map(i => pt(i.mapToItem(root, 0, 0))).join(" ")
    }

    // A binding: the Translate follows `p`. Starts at 10,10.
    Rectangle {
        id: bound
        x: 10; y: 10; width: 20; height: 20; color: "#ff0000"
        transform: Translate { x: root.p * 2; y: root.p / 2 }
    }
    // A Behavior on the Translate's x and y: it follows `p` in 400 ms. Starts at 10,50.
    Rectangle {
        id: easedItem
        x: 10; y: 50; width: 20; height: 20; color: "#00ff00"
        transform: Translate {
            id: te
            x: root.p; y: root.p / 4
            Behavior on x { NumberAnimation { duration: 400 } }
            Behavior on y { NumberAnimation { duration: 400 } }
        }
    }
    // An animation that names a Translate as its target. Starts at 10,90.
    Rectangle {
        id: runItem
        x: 10; y: 90; width: 20; height: 20; color: "#0000ff"
        transform: [ Scale { xScale: 2 }, Translate { id: tr } ]
    }
    NumberAnimation { id: run; target: tr; properties: "x,y"; from: 0; to: 60; duration: 200 }
    // `NumberAnimation on y` inside the Translate. Starts at 10,130.
    Rectangle {
        id: loopItem
        x: 10; y: 130; width: 20; height: 20; color: "#ffff00"
        transform: Translate {
            NumberAnimation on y { id: loop; running: false; from: 0; to: 40; duration: 200 }
        }
    }

    // Hover in a turned item: the 40x20 area shows at 125..145 x 20..60.
    Item {
        x: 150; y: 10; width: 60; height: 30
        transform: Rotation { angle: 90 }
        MouseArea {
            x: 10; y: 5; width: 40; height: 20
            hoverEnabled: true
            onEntered: root.say("turned-in")
            onExited: root.say("turned-out")
            onPositionChanged: mouse => root.say("turned " + root.r(mouse.x) + "," + root.r(mouse.y))
        }
    }
    // Hover in a scaled and moved item whose clip cuts the area: the item shows at
    // 200..280 x 110..130, the area in it at 220..280 x 110..130.
    Item {
        x: 200; y: 10; width: 40; height: 40
        clip: true
        transform: [ Scale { xScale: 2; yScale: 0.5 }, Translate { y: 100 } ]
        MouseArea {
            x: 10; width: 100; height: 100
            hoverEnabled: true
            onEntered: root.say("scaled-in")
            onExited: root.say("scaled-out")
            onPositionChanged: mouse => root.say("scaled " + root.r(mouse.x) + "," + root.r(mouse.y))
        }
    }
    // An area that a Translate slides under a pointer that stays where it is: at 200..230 x
    // 150..180, and 60 further left once `slide.x` is -60.
    MouseArea {
        id: slid
        x: 200; y: 150; width: 30; height: 30
        hoverEnabled: true
        transform: Translate { id: ts }
        onContainsMouseChanged: root.say("slid " + containsMouse)
    }
    // A drag inside a turned item: the pointer goes down the window, the knob along its
    // parent's x. The parent shows at 100..120 x 20..120, the knob at its top.
    Item {
        x: 120; y: 20; width: 100; height: 20
        transform: Rotation { angle: 90 }
        Rectangle {
            id: knob
            width: 20; height: 20; color: "#ffffff"
            MouseArea {
                anchors.fill: parent
                drag.target: knob; drag.axis: Drag.XAxis; drag.threshold: 0
                drag.minimumX: 0; drag.maximumX: 80
            }
        }
    }
}
