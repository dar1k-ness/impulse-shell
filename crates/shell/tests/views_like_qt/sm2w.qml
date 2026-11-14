import QtQuick
import Rustshell
Scope {
    id: root
    property var arr: ["a", "b", "c"]
    property var objs: [{id: 1, n: "x"}, {id: 2, n: "y"}]
    ScriptModel { id: sm; values: root.arr; onValuesChanged: console.log("valuesChanged", JSON.stringify(values)) }
    ScriptModel { id: so; values: root.objs; objectProp: "id" }
    ScriptModel { id: sn; values: root.objs }
    FloatingWindow { visible: true; implicitWidth: 200; implicitHeight: 400
    Item { id: host; width: 200; height: 400
        Repeater { model: sm; delegate: Item { required property var modelData; required property int index
            Component.onCompleted: console.log("rep +", modelData, index)
            Component.onDestruction: console.log("rep -", modelData)
            onIndexChanged: console.log("rep idx", modelData, index)
            onModelDataChanged: console.log("rep data", modelData, index) } }
        Repeater { model: so; delegate: Item { required property var modelData; required property int index
            Component.onCompleted: console.log("repo +", JSON.stringify(modelData), index)
            Component.onDestruction: console.log("repo -", JSON.stringify(modelData))
            onIndexChanged: console.log("repo idx", JSON.stringify(modelData), index)
            onModelDataChanged: console.log("repo data", JSON.stringify(modelData), index) } }
        Repeater { model: sn; delegate: Item { required property var modelData; required property int index
            Component.onCompleted: console.log("repn +", JSON.stringify(modelData), index)
            Component.onDestruction: console.log("repn -", JSON.stringify(modelData))
            onIndexChanged: console.log("repn idx", JSON.stringify(modelData), index)
            onModelDataChanged: console.log("repn data", JSON.stringify(modelData), index) } }
        ListView { width: 200; height: 400; model: sm; delegate: Item { height: 20; width: 200; required property var modelData; required property int index
            Component.onCompleted: console.log("lv +", modelData, index)
            Component.onDestruction: console.log("lv -", modelData)
            onIndexChanged: console.log("lv idx", modelData, index)
            onModelDataChanged: console.log("lv data", modelData, index) } }
    }
    }
    property var steps: [
        ["same content new array", () => root.arr = ["a", "b", "c"]],
        ["append", () => root.arr = ["a", "b", "c", "d"]],
        ["remove middle", () => root.arr = ["a", "c", "d"]],
        ["move", () => root.arr = ["d", "a", "c"]],
        ["replace one", () => root.arr = ["d", "X", "c"]],
        ["dups", () => root.arr = ["d", "d", "c", "d"]],
        ["clear", () => root.arr = []],
        ["numbers", () => root.arr = [1, 2, 3]],
        ["direct write", () => { sm.values = [3, 2, 1] }],
        ["objs same ids changed field", () => root.objs = [{id: 1, n: "x2"}, {id: 2, n: "y"}]],
        ["objs reorder + new", () => root.objs = [{id: 2, n: "y"}, {id: 3, n: "z"}, {id: 1, n: "x2"}]],
        ["objs same refs", () => { var o = root.objs; root.objs = [o[1], o[0], o[2]] }],
        ["undefined", () => root.arr = undefined]
    ]
    property int si: 0
    Timer { interval: 150; repeat: true; running: true; onTriggered: {
        if (root.si > 0) console.log("values", JSON.stringify(sm.values), sm.values ? sm.values.length : "no-length", JSON.stringify(so.values))
        if (root.si >= root.steps.length) { console.log("done"); running = false; Qt.quit(); return }
        console.log("--", root.steps[root.si][0]); root.steps[root.si][1](); root.si++
    } }
}
