// qt: start +20 -0 rows 0..19@0 tall 46 h 4798 | 350 +7 -0 rows 0..26@0 tall 46 h 4798 | 400 +1 -0 rows 0..27@0 tall 46 h 4798 | 800 +8 -9 rows 0..0@0,10..35@480 tall 46 h 4798
import QtQuick
import QtQuick.Layouts
Item {
    id: root
    width: 300; height: 600
    property string result: ""
    property int made: 0
    property int gone: 0
    property var steps: [350, 400, 800]
    property int k: -1
    property var was: [0, 0]
    property var out: []
    // The rows that have a delegate: runs of `index` with where the first of each run stands,
    // and the heights they have.
    function rows() {
        const l = []
        for (const c of v.contentItem.children)
            if (c.isRow && c.visible) l.push(c)
        l.sort((a, b) => a.index - b.index)
        const runs = []
        const tall = []
        for (let n = 0; n < l.length; n++) {
            if (tall.indexOf(l[n].height) < 0) tall.push(l[n].height)
            if (n && l[n].index === l[n - 1].index + 1) continue
            let m = n
            while (m + 1 < l.length && l[m + 1].index === l[m].index + 1) m++
            runs.push(l[n].index + ".." + l[m].index + "@" + l[n].y)
        }
        return runs.join(",") + " tall " + tall.join(",")
    }
    ListView {
        id: v
        width: 300; height: 600
        model: 100; spacing: 2
        // A row that takes its height from what is in it (a Column), where that item's implicit
        // height is bound to the layout's in it.
        delegate: Column {
            width: 300
            required property int index
            readonly property bool isRow: true
            Item {
                width: 300; implicitHeight: line.implicitHeight
                RowLayout {
                    id: line
                    Item { implicitWidth: 100; implicitHeight: 46 }
                }
            }
            Component.onCompleted: root.made++
            Component.onDestruction: root.gone++
        }
    }
    // Each step reads what the step before left, then scrolls.
    Timer {
        interval: 60; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + " +" + (root.made - root.was[0]) + " -" + (root.gone - root.was[1]) + " rows " + root.rows() + " h " + v.contentHeight)
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
