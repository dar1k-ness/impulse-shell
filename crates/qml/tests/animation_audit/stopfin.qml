import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property int n: 0
    property real a: 0
    property real b: 0
    property real c: 0
    property real d: 0
    property real e: 0
    property real f: 0
    property real g: 0
    NumberAnimation { id: na; target: root; property: "a"; to: 100; duration: 1000
        onStarted: say("a started"); onStopped: say("a stopped"); onFinished: say("a finished"); onRunningChanged: say("a running", running) }
    NumberAnimation { id: nb; target: root; property: "b"; to: 100; duration: 100; loops: Animation.Infinite
        onStarted: say("b started"); onStopped: say("b stopped"); onFinished: say("b finished"); onRunningChanged: say("b running", running) }
    SmoothedAnimation { id: nc; target: root; property: "c"; to: 100; velocity: 10
        onStarted: say("c started"); onStopped: say("c stopped"); onFinished: say("c finished"); onRunningChanged: say("c running", running) }
    SequentialAnimation { id: nd
        onStarted: say("d started"); onStopped: say("d stopped"); onFinished: say("d finished"); onRunningChanged: say("d running", running)
        SpringAnimation { target: root; property: "d"; to: 100; spring: 2; damping: 0.2 }
        PauseAnimation { duration: 50 } }
    ParallelAnimation { id: ne
        onStarted: say("e started"); onStopped: say("e stopped"); onFinished: say("e finished"); onRunningChanged: say("e running", running)
        SmoothedAnimation { target: root; property: "e"; to: 100; velocity: 10 }
        PauseAnimation { duration: 50 } }
    SpringAnimation { id: nf; target: root; property: "f"; to: 100; spring: 2; damping: 0.2; alwaysRunToEnd: true
        onStarted: say("f started"); onStopped: say("f stopped"); onFinished: say("f finished"); onRunningChanged: say("f running", running) }
    NumberAnimation { id: ng; target: root; property: "g"; to: 100; duration: 100; loops: 3
        onStarted: say("g started"); onStopped: say("g stopped"); onFinished: say("g finished"); onRunningChanged: say("g running", running) }
    Component.onCompleted: { na.start(); nb.start(); nc.start(); nd.start(); ne.start(); nf.start(); ng.start() }
    function sample() {
        n++
        if (n == 4) { say("stop"); na.stop(); nb.stop(); nc.stop(); nd.stop(); ne.stop(); nf.stop(); ng.stop(); say("stop done") }
        say("s", a.toFixed(2), b.toFixed(2), c.toFixed(2), d.toFixed(2), e.toFixed(2), f.toFixed(2), g.toFixed(2), nf.running)
    }
}
