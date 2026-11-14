import QtQuick
Item {
    id: root
    property string log: ""
    function ev(s) { log += s + "\n" }
    TextBase { id: a; out: root; onLengthChanged: root.ev("use len"); onTextChanged: root.ev("use text"); onCursorPositionChanged: root.ev("use cp") }
    TextBase { id: b; out: root; onTextChanged: root.ev("use text"); onLengthChanged: root.ev("use len") }
    Connections { target: b; function onLengthChanged() { root.ev("conn len") } function onTextChanged() { root.ev("conn text") } }
    Connections { target: b; function onTextChanged() { root.ev("conn2 text") } }
    // The same declarations made three times from one delegate.
    Repeater {
        id: rep
        model: 3
        Item {
            property alias t: t
            TextInput { id: t; onLengthChanged: root.ev("r" + index + " len"); onTextChanged: root.ev("r" + index + " text") }
            Connections { target: t; function onLengthChanged() { root.ev("r" + index + " conn len") } function onTextChanged() { root.ev("r" + index + " conn text") } }
            Connections { target: t; function onTextChanged() { root.ev("r" + index + " conn2 text") } function onLengthChanged() { root.ev("r" + index + " conn2 len") } }
        }
    }
    function run() {
        ev("-- a"); a.text = "hello"
        ev("-- b"); b.text = "hello"
        for (var i = 0; i < 3; i++) { ev("-- r" + i); rep.itemAt(i).t.text = "hello" }
    }
    Component.onCompleted: { run(); console.log("LOG<" + log + ">LOG"); Qt.quit() }
}
