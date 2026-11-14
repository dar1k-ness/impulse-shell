// qs: start: cur 0 item c0 rows c0,c1 told 0 undef 0 | emoji: cur 0 item e0 rows e0,e1,e2,e3,e4,e5,e6,e7,e8,e9,e10,e11 told 14,0 undef 0 | nopick: cur 14 item f14 rows f0,f1,f2,f3,f4,f5,f6,f7,f8,f9,f10,f11,f14 told 14 undef 0 | again: cur 0 item g0 rows g0,g1,g2,g3,g4,g5,g6,g7,g8,g9,g10,g11 told 0 undef 0 | same_keys: cur 14 item h14' rows h0',h1',h2',h3',h4',h5',h6',h7',h8',h9',h10',h11',h14' told 14 undef 0 | fewer: cur 0 item h0" rows h0",h1" told 1,0 undef 0
import QtQuick
import Quickshell
FloatingWindow {
    id: root
    width: 300; height: 120
    property string result: ""
    property var steps: ["emoji", "nopick", "again", "same_keys", "fewer"]
    property int k: -1
    property var out: []
    // The row the handler of `results` chooses once it gave the model its values, or -1.
    property int pick: -1
    property int gen: 0
    // Delegates whose `modelData` was taken away while they lived.
    property int undef: 0
    Component { id: row; QtObject { property string key; property string name } }
    function rows(prefix, n, tag) {
        const l = []
        for (let i = 0; i < n; i++) l.push(row.createObject(root, { key: prefix + i, name: prefix + i + tag }))
        return l
    }
    // The launcher's results: objects made anew for each query, the model keeps a row whose
    // `key` it had (SearchWidget.qml, `objectProp: "key"`).
    property var results: rows("c", 2, "")
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
            + " rows " + l.map(c => c.name).join(",") + " told " + told.join(",") + " undef " + root.undef
    }
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
            property QtObject entry: modelData
            onModelDataChanged: if (modelData === undefined) root.undef++
            readonly property string name: entry ? entry.name : "none"
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
                // Every row replaced by more rows than the view shows, the first one chosen:
                // the launcher's emoji after its clipboard entries.
                if (s === "emoji") { root.pick = 0; root.results = root.rows("e", 15, "") }
                // The same with no row chosen: the current row is left outside the rows shown.
                else if (s === "nopick") root.results = root.rows("f", 15, "")
                else if (s === "again") { root.pick = 0; root.results = root.rows("g", 15, "") }
                // New objects under the keys the model has, the current row outside the rows shown.
                else if (s === "same_keys") { root.results = root.rows("h", 15, ""); root.results = root.rows("h", 15, "'") }
                else { root.pick = 0; root.results = root.rows("h", 2, "\"") }
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
