// qt: start +19 -0 rows 0..18@0 h 4000 o 0 | 2000 +19 -18 rows 0..0@0,50..68@2000 h 4000 o 0 | 1990 +1 -1 rows 0..0@0,49..67@1960 h 4000 o 0 | 1900 +2 -2 rows 0..0@0,47..65@1880 h 4000 o 0 | 1700 +5 -5 rows 0..0@0,42..60@1680 h 4000 o 0 | 1000 +17 -17 rows 0..0@0,25..43@1000 h 4000 o 0 | 990 +1 -1 rows 0..0@0,24..42@960 h 4000 o 0 | 3600 +10 -19 rows 0..0@0,90..99@3600 h 4000 o 0 | 3500 +3 -0 rows 0..0@0,87..99@3480 h 4000 o 0
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int made: 0
    property int gone: 0
    property var steps: [2000, 1990, 1900, 1700, 1000, 990, 3600, 3500]
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
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + " +" + (root.made - root.was[0]) + " -" + (root.gone - root.was[1]) + " rows " + root.rows() + " h " + v.contentHeight + " o " + v.originY)
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
