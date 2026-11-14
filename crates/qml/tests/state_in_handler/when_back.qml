import QtQuick
// State B's `when` turns true in the handler of the change to A and false again while the
// Transition to A runs.
Item {
    id: root
    property string log: ""
    function say(...a) { log += a.join(" ") + "\n" }
    property bool a: false
    property bool b: false
    property real p: 0
    property int ticks: 0
    property bool done: false
    states: [
        State { name: "B"; when: root.b; PropertyChanges { target: root; p: 50 } },
        State { name: "A"; when: root.a; PropertyChanges { target: root; p: 100 } }
    ]
    transitions: Transition { NumberAnimation { property: "p"; duration: 320 } }
    onStateChanged: {
        say("state", state)
        if (state == "A" && !done) { done = true; b = true; say("b on in onStateChanged:", state) }
    }
    function sample() {
        ticks++
        if (ticks == 1) { a = true; say("after a:", state, "b", b) }
        if (ticks == 3) { b = false; say("tick 3:", state, "b", b) }
        if (ticks == 50) say("end:", state, p, "b", b)
    }
}
