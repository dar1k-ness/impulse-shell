import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property real lin: 0
    property real a: 0     // velocity 200 (default), 0 -> 100
    property real b: 0     // duration 200, default velocity: 0 -> 100
    property real c: 0     // velocity -1, duration 240
    property real d: 0     // maximumEasingTime 100, velocity 400, 0 -> 200
    property real e: 0     // maximumEasingTime 0
    property real f: 0     // reversingMode Immediate, retargeted
    property real g: 0     // reversingMode Sync, retargeted
    property real h: 0     // Eased (default), retargeted the same way forward
    property real k: 50    // explicit animation with from
    property bool re: false
    Behavior on a { SmoothedAnimation { id: sa; onRunningChanged: say("a running", running, root.a.toFixed(3)) } }
    Behavior on b { SmoothedAnimation { duration: 200 } }
    Behavior on c { SmoothedAnimation { velocity: -1; duration: 240 } }
    Behavior on d { SmoothedAnimation { velocity: 400; maximumEasingTime: 100 } }
    Behavior on e { SmoothedAnimation { velocity: 400; maximumEasingTime: 0 } }
    Behavior on f { SmoothedAnimation { velocity: 400; reversingMode: SmoothedAnimation.Immediate } }
    Behavior on g { SmoothedAnimation { velocity: 400; reversingMode: SmoothedAnimation.Sync } }
    Behavior on h { SmoothedAnimation { velocity: 400 } }
    SmoothedAnimation { id: sk; target: root; property: "k"; from: 10; to: 110; velocity: 400
        onStarted: say("k started", running); onStopped: say("k stopped", running, root.k); onFinished: say("k finished", running)
        onRunningChanged: say("k running", running, root.k.toFixed(3)) }
    NumberAnimation { id: ref; target: root; property: "lin"; from: 0; to: 1; duration: 640 }
    function sample() {
        say("s", lin.toFixed(3), a.toFixed(3), b.toFixed(3), c.toFixed(3), d.toFixed(3), e.toFixed(3), f.toFixed(3), g.toFixed(3), h.toFixed(3), k.toFixed(3))
        if (lin >= 0.2 && !re) { re = true; say("retarget"); f = -50; g = -50; h = 300 }
    }
    Component.onCompleted: { a = 100; b = 100; c = 100; d = 200; e = 200; f = 200; g = 200; h = 200; sk.start(); ref.start() }
}
