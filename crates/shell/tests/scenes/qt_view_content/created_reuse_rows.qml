// qt: start +19 -0 rows 0..18@0 pooled 0 reused 0 | 10 +0 -0 rows 0..18@0 pooled 0 reused 0 | 40 +1 -0 rows 0..19@0 pooled 0 reused 0 | 100 +1 -0 rows 0..20@0 pooled 0 reused 0 | 400 +8 -0 rows 0..28@0 pooled 0 reused 0 | 2000 +0 -0 rows 0..0@0,20..28@800,50..68@2000 pooled 28 reused 19 | 1900 +0 -0 rows 0..0@0,23..28@920,47..68@1880 pooled 31 reused 22 | 0 +0 -0 rows 0..18@0,56..65@2240 pooled 50 reused 40
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int made: 0
    property int gone: 0
    property var steps: [10, 40, 100, 400, 2000, 1900, 0]
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
        model: 100; reuseItems: true; property int pooled: 0; property int reused: 0
        delegate: Item {
            width: 300; height: 40; ListView.onPooled: v.pooled++; ListView.onReused: v.reused++
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
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + " +" + (root.made - root.was[0]) + " -" + (root.gone - root.was[1]) + " rows " + root.rows() + " pooled " + v.pooled + " reused " + v.reused)
            root.was = [root.made, root.gone]
            root.k++
            if (root.k < root.steps.length) {
                v.contentY = root.steps[root.k]
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
