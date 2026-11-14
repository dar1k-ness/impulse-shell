import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property real lin: 0
    property color c1: "#000000"      // string value in PropertyChanges
    property color c2: "#000000"      // Qt.rgba value
    property var c3: "#000000"        // var holding a string: not a colour property
    property string c4: "#000000"     // string property
    property real n1: 0               // NumberAnimation {} without properties
    property int n2: 0
    property real n3: 0               // only PropertyAnimation {} could take it
    property color c5: "#000000"      // ColorAnimation with targets but no properties
    Rectangle { id: r; color: "#000000"; border.color: "#000000" }
    Item { id: other; property color oc: "#000000"; property real on: 0 }
    states: State {
        name: "on"
        PropertyChanges { target: root; c1: "#ff0000"; c2: Qt.rgba(0, 1, 0, 1); c3: "#0000ff"; c4: "#00ff00"; n1: 100; n2: 100; n3: 100; c5: "blue" }
        PropertyChanges { target: r; color: "#804020"; border.color: "red" }
        PropertyChanges { target: other; oc: "#ffffff"; on: 50 }
    }
    transitions: Transition {
        ColorAnimation { duration: 160 }
        NumberAnimation { duration: 160 }
    }
    NumberAnimation { id: ref; target: root; property: "lin"; from: 0; to: 1; duration: 160 }
    function sample() { say("s", lin.toFixed(3), c1, c2, c3, c4, n1.toFixed(2), n2, n3.toFixed(2), c5, r.color, r.border.color, other.oc, other.on.toFixed(2)) }
    Component.onCompleted: { state = "on"; ref.start(); say("after", c1, c2, c3, c4, n1, n2, n3, c5, r.color, r.border.color, other.oc, other.on) }
}
