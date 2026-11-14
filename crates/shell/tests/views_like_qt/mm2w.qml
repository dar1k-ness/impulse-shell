import QtQuick
import Rustshell
Scope {
    id: root
    property var arr: []
    property var made: ({})
    ScriptModel { id: sm; values: root.arr }
    ListModel { id: lm }
    component Row: Item {
        id: row
        required property var modelData
        required property int index
        property string view
        property int uid: -1
        readonly property string name: (typeof modelData === "object" ? modelData.v : modelData) + "#" + uid
        width: 40; height: 20
        Component.onCompleted: { uid = root.made[view] = (root.made[view] ?? -1) + 1; console.log(view, "+", name, index) }
        Component.onDestruction: console.log(view, "-", name)
        onIndexChanged: console.log(view, "idx", name, index)
    }
    FloatingWindow { visible: true; implicitWidth: 400; implicitHeight: 400
    Item { width: 400; height: 400
        ListView { id: ls; width: 60; height: 200; model: sm
            onCurrentIndexChanged: console.log("ls current", currentIndex)
            onCurrentItemChanged: console.log("ls currentItem", currentItem ? currentItem.name : "none")
            onCountChanged: console.log("ls count", count)
            add: Transition { ScriptAction { script: console.log("ls add", ls.count) } }
            remove: Transition { ScriptAction { script: console.log("ls remove", ls.count) } }
            delegate: Row { view: "ls"
                ListView.onAdd: console.log("ls onAdd", name, index)
                ListView.onRemove: console.log("ls onRemove", name, index) } }
        ListView { id: ll; x: 60; width: 60; height: 200; model: lm
            onCurrentIndexChanged: console.log("ll current", currentIndex)
            onCurrentItemChanged: console.log("ll currentItem", currentItem ? currentItem.name : "none")
            onCountChanged: console.log("ll count", count)
            add: Transition { ScriptAction { script: console.log("ll add", ll.count) } }
            remove: Transition { ScriptAction { script: console.log("ll remove", ll.count) } }
            delegate: Row { view: "ll"
                ListView.onAdd: console.log("ll onAdd", name, index)
                ListView.onRemove: console.log("ll onRemove", name, index) } }
    }
    }
    function set(a) { root.arr = a }
    function both(i) { ls.currentIndex = i; ll.currentIndex = i }
    // Each step makes all its changes in one frame, the same ones on both models.
    property var steps: [
        ["fill, remove first, insert at 0", () => {
            set(["a", "b", "c"]); set(["b", "c"]); set(["x", "b", "c"])
            lm.append({v: "a"}); lm.append({v: "b"}); lm.append({v: "c"}); lm.remove(0); lm.insert(0, {v: "x"}) }],
        ["append two, one at a time", () => {
            set(["x", "b", "c", "d"]); set(["x", "b", "c", "d", "e"])
            lm.append({v: "d"}); lm.append({v: "e"}) }],
        ["move last to first, remove last", () => {
            set(["e", "x", "b", "c", "d"]); set(["e", "x", "b", "c"])
            lm.move(4, 0, 1); lm.remove(4) }],
        ["current 2", () => both(2)],
        ["remove first, insert two at 0", () => {
            set(["x", "b", "c"]); set(["p", "x", "b", "c"]); set(["q", "p", "x", "b", "c"])
            lm.remove(0); lm.insert(0, {v: "p"}); lm.insert(0, {v: "q"}) }],
        ["remove current, insert at its place", () => {
            set(["q", "p", "x", "c"]); set(["q", "p", "x", "B", "c"])
            lm.remove(3); lm.insert(3, {v: "B"}) }],
        ["move current to end, move it back", () => {
            set(["q", "p", "x", "c", "B"]); set(["q", "p", "x", "B", "c"])
            lm.move(3, 4, 1); lm.move(4, 3, 1) }],
        ["insert and remove the same row", () => {
            set(["q", "p", "z", "x", "B", "c"]); set(["q", "p", "x", "B", "c"])
            lm.insert(2, {v: "z"}); lm.remove(2) }],
        ["remove the last two, the current one first", () => {
            set(["q", "p", "x", "c"]); set(["q", "p", "x"])
            lm.remove(3); lm.remove(3) }],
        ["clear and fill", () => {
            set([]); set(["r", "s", "t"])
            lm.clear(); lm.append({v: "r"}); lm.append({v: "s"}); lm.append({v: "t"}) }],
        ["current 1", () => both(1)],
        ["clear, fill, remove first, insert at 0", () => {
            set([]); set(["a", "b", "c"]); set(["b", "c"]); set(["x", "b", "c"])
            lm.clear(); lm.append({v: "a"}); lm.append({v: "b"}); lm.append({v: "c"}); lm.remove(0); lm.insert(0, {v: "x"}) }],
        ["swap ends, then swap the middle in", () => {
            set(["c", "b", "x"]); set(["b", "c", "x"])
            lm.move(0, 2, 1); lm.move(0, 1, 1) }],
        ["change a row in place, then remove the one before", () => {
            set(["b", "C", "x"]); set(["C", "x"])
            lm.setProperty(1, "v", "C"); lm.remove(0) }],
        ["clear", () => { set([]); lm.clear() }],
        ["fill and clear", () => {
            set(["a"]); set([])
            lm.append({v: "a"}); lm.clear() }]
    ]
    property int si: 0
    Timer { interval: 150; repeat: true; running: true; onTriggered: {
        if (root.si > 0) console.log("now", JSON.stringify(sm.values), ls.currentIndex, ls.count, ll.currentIndex, ll.count)
        if (root.si >= root.steps.length) { console.log("done"); running = false; Qt.quit(); return }
        console.log("--", root.steps[root.si][0]); root.steps[root.si][1](); console.log("made")
        root.si++
    } }
}
