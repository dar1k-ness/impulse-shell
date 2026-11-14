// qt: start +14 -0 rows 0..13@0 w 4495 o 0 | 10 +1 -0 rows 0..14@0 w 4495 o 0 | 100 +2 -0 rows 0..16@0 w 4495 o 0 | 2000 +15 -16 rows 0..0@0,44..58@1980 w 4495 o 0 | 1900 +2 -2 rows 0..0@0,42..56@1890 w 4495 o 0 | 0 +13 -15 rows 0..13@0 w 4495 o 0
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int made: 0
    property int gone: 0
    property var steps: [10, 100, 2000, 1900, 0]
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
            runs.push(l[n].index + ".." + l[m].index + "@" + (l[n].x))
        }
        return runs.join(",") + (l.every(c => c.modelData === c.index) ? "" : " modelData!")
    }
    ListView {
        id: v
        width: 300; height: 400
        model: 100; orientation: ListView.Horizontal; spacing: 5
        delegate: Item {
            width: 40; height: 300
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
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + " +" + (root.made - root.was[0]) + " -" + (root.gone - root.was[1]) + " rows " + root.rows() + " w " + v.contentWidth + " o " + v.originX)
            root.was = [root.made, root.gone]
            root.k++
            if (root.k < root.steps.length) {
                v.contentX = root.steps[root.k]
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
