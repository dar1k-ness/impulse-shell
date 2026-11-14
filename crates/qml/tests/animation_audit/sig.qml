import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property real lin: 0
    property real x1: 0
    property real x2: 0
    property real x3: 0
    property real x4: 0
    property real x5: 0
    property real x6: 0
    property real x7: 7
    property int step: 0
    NumberAnimation { id: a1; target: root; property: "x1"; from: 0; to: 10; duration: 80; loops: 3
        onStarted: say("a1 started", running, root.x1)
        onStopped: say("a1 stopped", running, root.x1)
        onFinished: say("a1 finished", running, root.x1)
        onRunningChanged: say("a1 running", running, root.x1) }
    NumberAnimation { id: a2; target: root; property: "x2"; from: 0; to: 10; duration: 80; loops: 3; alwaysRunToEnd: true
        onStarted: say("a2 started", running, root.x2)
        onStopped: say("a2 stopped", running, root.x2)
        onFinished: say("a2 finished", running, root.x2)
        onRunningChanged: say("a2 running", running, root.x2) }
    NumberAnimation { id: a3; target: root; property: "x3"; from: 0; to: 100; duration: 160
        onStarted: say("a3 started", running, paused)
        onStopped: say("a3 stopped", running, paused, root.x3)
        onFinished: say("a3 finished", running, paused)
        onRunningChanged: say("a3 running", running, paused)
        onPausedChanged: say("a3 paused", paused, running, root.x3) }
    SequentialAnimation { id: a4
        onStarted: say("a4 started", running)
        onStopped: say("a4 stopped", running, root.x4)
        onFinished: say("a4 finished", running)
        onRunningChanged: say("a4 running", running)
        NumberAnimation { id: a4c; target: root; property: "x4"; from: 0; to: 10; duration: 32
            onStarted: say("a4c started"); onStopped: say("a4c stopped"); onFinished: say("a4c finished"); onRunningChanged: say("a4c running", running) }
        ScriptAction { script: say("a4 script", a4.running, root.x4) }
        PauseAnimation { duration: 16 }
    }
    NumberAnimation { id: a5; target: root; property: "x5"; from: 0; to: 10; duration: 48; running: true
        onStarted: say("a5 started", running, root.x5)
        onStopped: say("a5 stopped", running, root.x5)
        onFinished: say("a5 finished", running, root.x5)
        onRunningChanged: say("a5 running", running, root.x5) }
    Behavior on x6 { NumberAnimation { id: a6; duration: 48
        onStarted: say("a6 started", running)
        onStopped: say("a6 stopped", running, root.x6)
        onFinished: say("a6 finished", running)
        onRunningChanged: say("a6 running", running, root.x6) } }
    NumberAnimation { id: a7; target: root; property: "x7"; to: 10; duration: 0
        onStarted: say("a7 started", running, root.x7)
        onStopped: say("a7 stopped", running, root.x7)
        onFinished: say("a7 finished", running, root.x7)
        onRunningChanged: say("a7 running", running, root.x7) }
    NumberAnimation { id: ref; target: root; property: "lin"; from: 0; to: 1; duration: 800 }
    function sample() {
        step++
        say("s", step, x1.toFixed(2), x2.toFixed(2), x3.toFixed(2), x4.toFixed(2), x5.toFixed(2), x6.toFixed(2))
        if (step == 2) { say("-- complete a1"); a1.complete(); say("after", a1.running, x1.toFixed(2)); say("-- stop a2"); a2.stop(); say("after", a2.running, x2.toFixed(2)) }
        if (step == 3) { say("-- pause a3"); a3.pause(); say("after", a3.running, a3.paused, x3.toFixed(2)); say("-- pause a3 again"); a3.pause() }
        if (step == 4) { say("-- start a2 while finishing"); a2.start(); say("after", a2.running) }
        if (step == 6) { say("-- resume a3"); a3.resume(); say("after", a3.running, a3.paused) }
        if (step == 8) { say("-- paused = true"); a3.paused = true; say("after", a3.paused) }
        if (step == 9) { say("-- complete paused a3"); a3.complete(); say("after", a3.running, a3.paused, x3.toFixed(2)) }
        if (step == 10) { say("-- pause stopped a3"); a3.pause(); say("after", a3.running, a3.paused); a3.paused = true; say("after2", a3.paused) }
        if (step == 11) { say("-- zero duration"); a7.start(); say("after", a7.running, x7) }
        if (step == 12) { say("-- x6 again"); x6 = 0 }
        if (step == 14) { say("-- child start"); a4c.start(); say("after", a4c.running) }
        if (step == 16) { say("-- restart seq, stop"); a4.restart(); a4.stop(); say("after", a4.running, x4) }
        if (step == 18) { say("-- a2 stop, restart"); a2.stop(); a2.restart(); say("after", a2.running, x2.toFixed(2)) }
    }
    Component.onCompleted: { say("completed", a5.running, x5); a1.start(); a2.start(); a3.start(); a4.start(); x6 = 10; ref.start(); say("end completed") }
}
