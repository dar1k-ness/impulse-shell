import QtQuick
import QtQml.Models
Item {
    id: root
    width: 200; height: 200
    property var objs: [{n: "x", k: 1}, {n: "y", k: 2}]
    property var strs: ["a", "b"]
    Repeater {
        model: root.objs
        delegate: Item {
            required property var modelData; required property int index; required property string n; required property int k; required property var model
            property string lbl: "L" + index + n
            onModelDataChanged: console.log("rp data", index, n, k, typeof model, "[" + lbl + "]")
            onIndexChanged: console.log("rp idx", index, n, k, typeof model, "[" + lbl + "]")
            onNChanged: console.log("rp n", index, n, k, typeof modelData, typeof model)
            onKChanged: console.log("rp k", index, n, k, typeof modelData, typeof model)
            onModelChanged: console.log("rp model", index, n, k, typeof modelData)
            onLblChanged: console.log("rp lbl", lbl)
            Component.onCompleted: console.log("rp +", index, n, k, lbl)
        }
    }
    ListView {
        width: 100; height: 100
        model: ListModel { ListElement { n: "p"; k: 5 } ListElement { n: "q"; k: 6 } }
        delegate: Item {
            width: 100; height: 20
            required property int index; required property string n; required property int k; required property var model
            onIndexChanged: console.log("lv idx", index, n, k, typeof model)
            onNChanged: console.log("lv n", index, n, k, typeof model)
            onKChanged: console.log("lv k", index, n, k, typeof model)
            onModelChanged: console.log("lv model", index, n, k)
            Component.onCompleted: console.log("lv +", index, n, k)
        }
    }
    Instantiator {
        model: root.strs
        delegate: QtObject {
            required property int index; required property var modelData
            property string lbl: "L" + index + modelData
            onIndexChanged: console.log("in idx", index, modelData, "[" + lbl + "]")
            onModelDataChanged: console.log("in data", index, modelData, "[" + lbl + "]")
            onLblChanged: console.log("in lbl", lbl)
            Component.onCompleted: console.log("in +", index, modelData, lbl)
        }
    }
    Timer { interval: 100; running: true; onTriggered: { console.log("-- set"); root.strs = ["a", "c", "b"]; console.log("-- objs"); root.objs = [{n: "y", k: 2}, {n: "z", k: 3}]; console.log("end") } }
}
