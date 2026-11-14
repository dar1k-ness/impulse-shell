// qt: start +19 -0 rows 0..18@0 y 0 at20 none at60 none | p50 +19 -18 rows 0..0@0,50..68@2000 y 2000 at20 none at60 60 | p99 +10 -19 rows 0..0@0,90..99@3600 y 3600 at20 none at60 none | p0 +18 -10 rows 0..18@0 y 0 at20 none at60 none | e60 +20 -19 rows 0..0@0,51..69@2040 y 2040 at20 none at60 60 | c30 +19 -19 rows 0..0@0,25..43@1000 y 1020 at20 none at60 none | v80 +20 -20 rows 0..0@0,71..89@2840 y 2840 at20 none at60 none | n5 +19 -19 rows 0..0@0,5..23@200 y 200 at20 20 at60 none | n40 +20 -20 rows 0..0@0,31..49@1240 y 1240 at20 none at60 none
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int made: 0
    property int gone: 0
    property var steps: ["p50", "p99", "p0", "e60", "c30", "v80", "n5", "n40"]
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
        model: 100
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
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + " +" + (root.made - root.was[0]) + " -" + (root.gone - root.was[1]) + " rows " + root.rows() + " y " + v.contentY + " at20 " + (v.itemAtIndex(20) ? v.itemAtIndex(20).index : "none") + " at60 " + (v.itemAtIndex(60) ? v.itemAtIndex(60).index : "none"))
            root.was = [root.made, root.gone]
            root.k++
            if (root.k < root.steps.length) {
                const s = root.steps[root.k], n = +s.slice(1)
                v.positionViewAtIndex(n, {p: ListView.Beginning, e: ListView.End, c: ListView.Center, v: ListView.Visible, n: ListView.Contain}[s[0]])
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
