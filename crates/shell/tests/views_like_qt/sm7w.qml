import QtQuick
import Rustshell
Scope {
    id: root
    property var arr: ["a", "b", "c"]
    property int n: 0
    ScriptModel { id: sm; values: root.arr; onValuesChanged: console.log("valuesChanged", JSON.stringify(values)) }
    FloatingWindow { visible: true; implicitWidth: 200; implicitHeight: 400
    Item { id: host; width: 200; height: 400
        ListView { id: lv; width: 200; height: 400; model: sm
            onCurrentIndexChanged: console.log("current", currentIndex)
            onCurrentItemChanged: console.log("currentItem", currentItem ? currentItem.modelData + "#" + currentItem.uid : "none")
            onCountChanged: console.log("count", count)
            delegate: Item { height: 20; width: 200; required property var modelData; required property int index
            property int uid: -1
            Component.onCompleted: { uid = root.n++; console.log("lv +", modelData, index, uid) }
            Component.onDestruction: console.log("lv -", modelData, uid)
            ListView.onIsCurrentItemChanged: console.log("isCurrent", modelData, uid, ListView.isCurrentItem)
            onIndexChanged: console.log("lv idx", modelData, uid, index) } }
    }
    }
    property var steps: [
        ["append", () => root.arr = ["a", "b", "c", "d"]],
        ["remove middle", () => root.arr = ["a", "c", "d"]],
        ["move", () => root.arr = ["d", "a", "c"]],
        ["replace one", () => root.arr = ["d", "X", "c"]],
        ["dups", () => root.arr = ["d", "d", "c", "d"]],
        ["remove first two", () => root.arr = ["c", "d"]],
        ["prepend", () => root.arr = ["p", "q", "c", "d"]],
        ["remove current", () => root.arr = ["p", "q", "d"]],
        ["remove last current", () => root.arr = ["p", "q"]],
        ["swap", () => root.arr = ["q", "p"]],
        ["clear", () => root.arr = []],
        ["fill", () => root.arr = [1, 2, 3, 4, 5]],
        ["set current 3", () => lv.currentIndex = 3],
        ["remove around", () => root.arr = [1, 4, 5]],
        ["move current first", () => root.arr = [4, 1, 5]],
        ["clear again", () => root.arr = []],
        ["undefined", () => root.arr = undefined]
    ]
    property int si: 0
    Timer { interval: 150; repeat: true; running: true; onTriggered: {
        if (root.si > 0) console.log("values", JSON.stringify(sm.values), lv.currentIndex, lv.count)
        if (root.si >= root.steps.length) { console.log("done"); running = false; Qt.quit(); return }
        console.log("--", root.steps[root.si][0]); root.steps[root.si][1](); root.si++
    } }
}
