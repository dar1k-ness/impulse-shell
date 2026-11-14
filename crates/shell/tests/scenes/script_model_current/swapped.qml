// qs: start: cur 0 item a0 focus a0 rows a0,b0,c0 told 0 | swap_first: cur 0 item a1 focus a1 rows a1,b1,c1 told 2,0 | swap: cur 2 item c2 focus c2 rows a2,b2,c2 told 2 | swap_same: cur 2 item c3 focus c3 rows a3,b3,c3 told  | pick_swap: cur 2 item c4 focus c4 rows a4,b4,c4 told 1,2 | swap_other: cur 2 item c6 focus c6 rows a6,b6,c6 told 1,2 | typed: cur 0 item a10 focus a10 rows a10,b10,c10 told 0,2,0,2,0,2,0 | direct: cur 1 item q focus q rows p,q,r told 2,0,2,1
import QtQuick
import Quickshell
FloatingWindow {
    id: root
    width: 300; height: 400
    property string result: ""
    property var steps: ["swap_first", "swap", "swap_same", "pick_swap", "swap_other", "typed", "direct"]
    property int k: -1
    property var out: []
    // The row the handler of `results` chooses once it gave the model its values, or -1.
    property int pick: -1
    property int gen: 0
    // Values none of which the model had: ScriptModel::updateValuesUnique puts the new rows
    // in before the old ones, then takes the old ones out.
    property var results: ["a", "b", "c"].map(c => c + root.gen)
    // The values come from the change handler of a bound property, as the launcher's search
    // results do (SearchWidget.qml).
    onResultsChanged: {
        sm.values = root.results
        if (root.pick >= 0) v.currentIndex = root.pick
    }
    function at() {
        const l = []
        for (const c of v.contentItem.children)
            if (c.isRow) l.push(c)
        l.sort((a, b) => a.index - b.index)
        const told = v.told
        v.told = []
        return "cur " + v.currentIndex + " item " + (v.currentItem ? v.currentItem.name : "none")
            + " focus " + l.filter(c => c.focus).map(c => c.name).join(",")
            + " rows " + l.map(c => c.name).join(",") + " told " + told.join(",")
    }
    ListView {
        id: v
        property var told: []
        onCurrentIndexChanged: told.push(currentIndex)
        width: 300; height: 400
        model: ScriptModel { id: sm; values: ["a0", "b0", "c0"] }
        delegate: Item {
            width: 300; height: 40
            required property var modelData
            required property int index
            readonly property string name: modelData
            readonly property bool isRow: true
        }
    }
    // Each step reads what the step before left, then does its own.
    Timer {
        interval: 60; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + ": " + root.at())
            root.k++
            if (root.k < root.steps.length) {
                const s = root.steps[root.k]
                root.pick = -1
                // The first row is chosen right after the rows changed.
                if (s === "swap_first") { root.pick = 0; root.gen++ }
                else if (s === "swap") root.gen++
                // The row the current one came to be is chosen.
                else if (s === "swap_same") { root.pick = 2; root.gen++ }
                else if (s === "pick_swap") { v.currentIndex = 1; root.gen++ }
                else if (s === "swap_other") { root.pick = 1; root.gen++; root.pick = -1; root.gen++ }
                // A word typed letter by letter, the first row chosen after each.
                else if (s === "typed") { root.pick = 0; for (let n = 0; n < 4; n++) root.gen++ }
                // The same from a plain handler, no binding in between.
                else { sm.values = ["x", "y", "z"]; v.currentIndex = 0; sm.values = ["p", "q", "r"]; v.currentIndex = 1 }
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
