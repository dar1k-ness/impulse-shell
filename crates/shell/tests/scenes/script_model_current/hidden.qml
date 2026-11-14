// qs: start: cur 0 item c0 rows c0,c1 told 0 | hide: cur 0 item c0 rows - told  | pick: cur 0 item e0 rows - told 14,0 | show: cur 0 item e0 rows e0,e1,e2,e3,e4,e5,e6,e7,e8,e9,e10,e11 told  | hide: cur 0 item e0 rows - told  | twice: cur 0 item g0 rows - told 4,0,14,0 | show: cur 0 item g0 rows g0,g1,g2,g3,g4,g5,g6,g7,g8,g9,g10,g11 told 
import QtQuick
import Quickshell
FloatingWindow {
    id: root
    width: 300; height: 120
    property string result: ""
    property var steps: ["hide", "pick", "show", "hide", "twice", "show"]
    property int k: -1
    property var out: []
    // The row the handler of `results` chooses once it gave the model its values, or -1.
    property int pick: -1
    Component { id: row; QtObject { property string key; property string name } }
    function rows(prefix, n) {
        const l = []
        for (let i = 0; i < n; i++) l.push(row.createObject(root, { key: prefix + i, name: prefix + i }))
        return l
    }
    property var results: rows("c", 2)
    onResultsChanged: {
        sm.values = root.results
        if (root.pick >= 0) v.currentIndex = root.pick
    }
    function at() {
        const l = []
        for (const c of v.contentItem.children)
            if (c.isRow) l.push(c)
        // Rows the model let go all have the index -1.
        l.sort((a, b) => a.index - b.index || (a.name < b.name ? -1 : 1))
        const told = v.told
        v.told = []
        return "cur " + v.currentIndex + " item " + (v.currentItem ? v.currentItem.name : "none")
            + " rows " + (sub.visible ? l.map(c => c.name).join(",") : "-") + " told " + told.join(",")
    }
    // The launcher's window: shown once, then hidden while its search gets the rows of the
    // query it opens with next. The rows are read while it shows: hidden, ours makes the rows
    // past the view's end too, until the window shows.
    FloatingWindow {
        id: sub
        width: 300; height: 120
        ListView {
            id: v
            property var told: []
            onCurrentIndexChanged: told.push(currentIndex)
            width: 300; height: 120
            model: ScriptModel { id: sm; objectProp: "key" }
            delegate: Item {
                width: 300; height: 40
                required property var modelData
                required property int index
                readonly property string name: modelData ? modelData.name : "none"
                readonly property bool isRow: true
            }
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
                // Every row replaced while the window is hidden, the first one chosen.
                if (s === "pick") { root.pick = 0; root.results = root.rows("e", 15) }
                else if (s === "show") sub.visible = true
                else if (s === "hide") sub.visible = false
                // Replaced twice before the window shows: the launcher's first rows, then all.
                else { root.pick = 0; root.results = root.rows("f", 5); root.results = root.rows("g", 15) }
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
