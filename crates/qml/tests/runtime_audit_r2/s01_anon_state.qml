import QtQuick
Item {
    id: r
    property bool on: false
    property int v: 1
    states: State { when: r.on; PropertyChanges { target: r; v: 2 } }
    Component.onCompleted: { on = true; console.log("v", v, "state", JSON.stringify(state)); on = false; console.log("v", v, "state", JSON.stringify(state)) }
}
