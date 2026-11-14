import QtQuick
// Handlers that are not the changed object's own: a Connections, a child's handler that reads
// a binding of its parent, a handler of the parent on a child's property.
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property int a: 0
    property int b: a * 2
    property int sum: kid.k + 1
    property bool both: one.on && two.on
    property int frame: 0
    onAChanged: say("own a", a, "b", b)
    onBChanged: say("own b", b)
    onSumChanged: say("own sum", sum)
    onBothChanged: say("own both", both)
    Connections {
        target: root
        function onAChanged() { root.say("conn a", root.a, "b", root.b) }
        function onBChanged() { root.say("conn b", root.b) }
    }
    Item {
        id: kid
        property int k: 0
        property int twice: root.a * 2
        onKChanged: root.say("kid k", k, "sum", root.sum)
        onTwiceChanged: root.say("kid twice", twice, "b", root.b)
    }
    Item { id: one; property bool on: false; onOnChanged: root.look("one") }
    Item { id: two; property bool on: false; onOnChanged: root.look("two") }
    Connections {
        target: kid
        function onKChanged() { root.say("conn k", kid.k, "sum", root.sum) }
    }
    function look(who) { say("look", who, one.on, two.on, "both", both) }
    function sample() {
        frame++
        if (frame === 2) { say("write a"); a = 1; say("written", b, kid.twice) }
        if (frame === 3) { say("write k"); kid.k = 5; say("written", sum) }
        if (frame === 4) { say("write one"); one.on = true; say("write two"); two.on = true; say("written", both) }
    }
}
