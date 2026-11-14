// qt: start +14 -0 rows 0..13@0 h 5297 o 0 | 100 +3 -0 rows 0..16@0 h 4905 o 0 | 2000 +15 -16 rows 0..0@-161,40..54@1968 h 5402 o -192 | 1900 +2 -2 rows 0..0@-132,38..52@1852 h 5297 o -162 | 4000 +15 -15 rows 0..0@-55,78..92@3972 h 5192 o -84 | 0 +14 -15 rows 0..14@-41 h 5297 o -71
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int made: 0
    property int gone: 0
    property var steps: [100, 2000, 1900, 4000, 0]
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
        model: 100; spacing: 3
        delegate: Item {
            width: 300; height: 20 + (index % 7) * 10
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
