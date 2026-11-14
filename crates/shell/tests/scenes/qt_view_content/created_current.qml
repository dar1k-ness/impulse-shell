// qt: start +19 -0 rows 0..18@0 cur 3@120 | y2000 +19 -18 rows 3..3@120,50..68@2000 cur 3@120 | c50 +0 -1 rows 50..68@2000 cur 50@2000 | y1000 +19 -18 rows 25..43@1000,50..50@2000 cur 50@2000 | c70 +1 -1 rows 25..43@1000,70..70@2800 cur 70@2800 | y0 +19 -19 rows 0..18@0,70..70@2800 cur 70@2800 | c-1 +0 -1 rows 0..18@0 cur none
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int made: 0
    property int gone: 0
    property var steps: ["y2000", "c50", "y1000", "c70", "y0", "c-1"]
    property int k: -1
    property var was: [0, 0]
    property var out: []
    // The rows that have a delegate, as runs of `index`, and where the first of them stands.
    function rows() {
        const l = []
        for (const c of v.contentItem.children)
            if (c.isRow && c.visible) l.push(c)
        l.sort((a, b) => a.index - b.index)
        const runs = []
        for (let n = 0; n < l.length; n++) {
            if (n && l[n].index === l[n - 1].index + 1) continue
            let m = n
            while (m + 1 < l.length && l[m + 1].index === l[m].index + 1) m++
            runs.push(l[n].index + ".." + l[m].index + "@" + (l[n].y))
        }
        return runs.join(",") + (l.every(c => c.modelData === c.index) ? "" : " modelData!")
    }
    ListView {
        id: v
        width: 300; height: 400
        model: 100; currentIndex: 3
        delegate: Item {
            width: 300; height: 40
            required property int index
            required property var modelData
            readonly property bool isRow: true
            Component.onCompleted: root.made++
            Component.onDestruction: root.gone++
        }
    }
    // Each step reads what the step before left, then does its own.
    Timer {
        interval: 60; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + " +" + (root.made - root.was[0]) + " -" + (root.gone - root.was[1]) + " rows " + root.rows() + " cur " + (v.currentItem ? v.currentItem.index + "@" + v.currentItem.y : "none"))
            root.was = [root.made, root.gone]
            root.k++
            if (root.k < root.steps.length) {
                const s = root.steps[root.k]; if (s[0] === "y") v.contentY = +s.slice(1); else v.currentIndex = +s.slice(1)
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
