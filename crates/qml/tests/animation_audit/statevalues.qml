import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property real lin: 0
    property var v1: "black"
    property string s1: "black"
    property color c1: "black"
    property real n1: 0
    property real n2: 0
    Rectangle { id: r; color: "#000000"; border.color: "#000000" }
    states: State {
        name: "on"
        PropertyChanges { target: root; v1: "red"; s1: "red"; c1: "red"; n1: 100; n2: 100 }
        PropertyChanges { target: r; border.color: "red" }
    }
    transitions: Transition {
        ColorAnimation { properties: "v1,s1,c1,border.color"; duration: 96 }
        NumberAnimation { property: "n1"; to: 40; duration: 96 }
        NumberAnimation { property: "n2"; from: 100; to: 100; duration: 96 }
    }
    NumberAnimation { id: ref; target: root; property: "lin"; from: 0; to: 1; duration: 160 }
    function sample() { say("s", lin.toFixed(3), v1, typeof v1, s1, c1, r.border.color, n1.toFixed(2), n2.toFixed(2)) }
    Component.onCompleted: { state = "on"; ref.start(); say("after", v1, s1, c1, r.border.color, n1, n2) }
}
