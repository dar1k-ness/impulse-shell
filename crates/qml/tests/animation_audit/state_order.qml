import QtQuick
// Order inside a state switch with a Transition, and inside the frames that follow.
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property real p: 0
    property real q: 0
    property real np: 0
    property color col: "#000000"
    property real sum: p + q
    property int ticks: 0
    // Keeps Qt's animation timer going between the transitions: stopped, it restarts from the
    // wall clock, which the stepped clock of the harness has left behind.
    property real keep: 0
    NumberAnimation on keep { from: 0; to: 1; duration: 1000000 }
    function snap() { return state + "|" + [p, q, np].map(v => v.toFixed(0)).join(" ") + " " + col + " tr=" + tr.running }
    states: [
        State {
            name: "on"
            PropertyChanges { target: root; p: 80; q: 160; np: 5; col: "#ff0000" }
        },
        State {
            name: "far"
            PropertyChanges { target: root; p: 800; np: 9 }
        }
    ]
    transitions: Transition {
        id: tr
        NumberAnimation { properties: "p,q"; duration: 80 }
        onRunningChanged: say("tr.running", tr.running, root.snap())
    }
    onStateChanged: say("state     ", snap())
    onPChanged: say("p         ", snap())
    onQChanged: say("q         ", snap())
    onNpChanged: say("np        ", snap())
    onColChanged: say("col       ", snap())
    onSumChanged: say("sum       ", snap(), sum)
    function sample() {
        ticks++
        say("end       ", snap())
        if (ticks == 1) { state = "on"; say("after on  ", snap()) }
        if (ticks == 3) { state = ""; say("after ''  ", snap()) }
        if (ticks == 12) { state = "on"; say("after on  ", snap()) }
        if (ticks == 22) { state = "far"; say("after far ", snap()) }
        if (ticks == 24) { state = "on"; say("after on  ", snap()) }
    }
}
