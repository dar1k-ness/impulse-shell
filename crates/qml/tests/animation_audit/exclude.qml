import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property real lin: 0
    property color c1: "#000000"
    property color c2: "#000000"
    property real n1: 0
    Item { id: other; property color oc: "#000000"; property color od: "#000000" }
    states: State {
        name: "on"
        PropertyChanges { target: root; c1: "#ff0000"; c2: "#00ff00"; n1: 100 }
        PropertyChanges { target: other; oc: "#ffffff"; od: "#ffffff" }
    }
    transitions: Transition {
        ColorAnimation { target: other; duration: 160 }                 // target, no property: every colour of `other`
        ColorAnimation { properties: "c1"; duration: 80 }
        ColorAnimation { exclude: other; duration: 160 }                   // every colour but those of `other`
        PropertyAnimation { duration: 160 }                              // no property: nothing
    }
    NumberAnimation { id: ref; target: root; property: "lin"; from: 0; to: 1; duration: 160 }
    function sample() { say("s", lin.toFixed(3), c1, c2, n1.toFixed(2), other.oc, other.od) }
    Component.onCompleted: { state = "on"; ref.start(); say("after", c1, c2, n1, other.oc, other.od) }
}
