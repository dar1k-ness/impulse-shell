import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property int n: 0
    property real a: 0     // spring 2, damping 0.2: 0 -> 100
    property real b: 0     // the same with mass 2
    property real c: 0     // the same with epsilon 5
    property real d: 0     // spring 5, damping 0.05, velocity 100: the speed is capped
    property real e: 0     // velocity 200 alone: straight there
    property real f: 0     // nothing set: nothing moves
    property real g: 350   // spring, modulus 360: 350 -> 10 through 360
    property real h: 350   // velocity, modulus 360: 350 -> 10 through 360
    property real i: 0     // spring, retargeted on the way
    property real j: 0     // velocity, retargeted on the way
    property real k: 50    // on its own, with from: stopped on the way and started again
    property real l: 0     // on its own, without from: stopped on the way and started again
    property real m: 0     // damping 3, mass 0
    property real o: 0     // spring -1, velocity 300
    property int p: 0      // an int property
    property real q: 0     // in a SequentialAnimation, a NumberAnimation after it
    property real r: 0     // in a Transition
    Behavior on a { SpringAnimation { id: sa; spring: 2; damping: 0.2; onRunningChanged: say("a running", running, root.a.toFixed(3)) } }
    Behavior on b { SpringAnimation { spring: 2; damping: 0.2; mass: 2 } }
    Behavior on c { SpringAnimation { spring: 2; damping: 0.2; epsilon: 5 } }
    Behavior on d { SpringAnimation { spring: 5; damping: 0.05; velocity: 100 } }
    Behavior on e { SpringAnimation { id: se; velocity: 200; onRunningChanged: say("e running", running, root.e.toFixed(3)) } }
    Behavior on f { SpringAnimation { id: sf } }
    Behavior on g { SpringAnimation { spring: 2; damping: 0.2; modulus: 360 } }
    Behavior on h { SpringAnimation { velocity: 200; modulus: 360 } }
    Behavior on i { SpringAnimation { spring: 2; damping: 0.2 } }
    Behavior on j { SpringAnimation { velocity: 200 } }
    SpringAnimation { id: sk; target: root; property: "k"; from: 10; to: 110; spring: 3; damping: 0.3
        onStarted: say("k started", running); onStopped: say("k stopped", running, root.k.toFixed(3)); onFinished: say("k finished", running)
        onRunningChanged: say("k running", running, root.k.toFixed(3)) }
    SpringAnimation { id: sl; target: root; property: "l"; to: 100; spring: 3; damping: 0.3 }
    Behavior on m { SpringAnimation { id: sm; spring: 2; damping: 3; mass: 0 } }
    Behavior on o { SpringAnimation { spring: -1; velocity: 300 } }
    Behavior on p { SpringAnimation { spring: 2; damping: 0.2 } }
    SequentialAnimation { id: sq
        SpringAnimation { target: root; property: "q"; to: 40; velocity: 250 }
        NumberAnimation { target: root; property: "q"; to: 0; duration: 160 }
        onFinished: say("q finished", root.q.toFixed(3))
    }
    states: State { name: "on"; PropertyChanges { root.r: 100 } }
    transitions: Transition { SpringAnimation { property: "r"; spring: 4; damping: 0.4 } }
    function sample() {
        n++
        say("s", a.toFixed(3), b.toFixed(3), c.toFixed(3), d.toFixed(3), e.toFixed(3), f.toFixed(3), g.toFixed(3), h.toFixed(3))
        say("t", i.toFixed(3), j.toFixed(3), k.toFixed(3), l.toFixed(3), m.toFixed(3), o.toFixed(3), p, q.toFixed(3), r.toFixed(3))
        if (n == 6) { say("retarget"); i = -50; j = -50; sk.stop(); sl.stop() }
        if (n == 9) { say("again"); sk.start(); sl.start() }
        if (n == 12) { state = "" }
    }
    Component.onCompleted: {
        say("props", sm.damping, sm.mass, sa.epsilon, sa.modulus, sa.velocity, sf.spring, sf.damping, sf.mass)
        a = 100; b = 100; c = 100; d = 100; e = 100; f = 100; g = 10; h = 10; i = 100; j = 100; m = 100; o = 100; p = 100
        sk.start(); sl.start(); sq.start(); state = "on"
    }
}
