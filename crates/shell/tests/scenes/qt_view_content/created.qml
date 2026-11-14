// qt: start +19 -0 rows 0..18 | y10 +0 -0 rows 0..18 | y40 +1 -0 rows 0..19 | y100 +1 -0 rows 0..20 | y400 +8 -0 rows 0..28 | y2000 +19 -28 rows 0..68 | y0 +18 -19 rows 0..18
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int made: 0
    property int gone: 0
    property var live: []
    property var ys: [10, 40, 100, 400, 2000, 0]
    property int k: -1
    property var was: [0, 0]
    property var out: []
    function span() { const l = live.slice().sort((a, b) => a - b); return l.length ? l[0] + ".." + l[l.length - 1] : "-" }
    ListView {
        id: v
        width: 300; height: 400
        model: 100
        // cacheBuffer is 320 unless set
        delegate: Item {
            width: 300; height: 40
            // `index` is gone by the time the delegate is destroyed.
            property int row: -1
            Component.onCompleted: { row = index; root.made++; root.live.push(row) }
            Component.onDestruction: { root.gone++; root.live.splice(root.live.indexOf(row), 1) }
        }
    }
    // Each step reads what the write of the step before left, then scrolls on.
    Timer {
        interval: 60; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : "y" + root.ys[root.k]) + " +" + (root.made - root.was[0]) + " -" + (root.gone - root.was[1]) + " rows " + root.span())
            root.was = [root.made, root.gone]
            root.k++
            if (root.k < root.ys.length) {
                v.contentY = root.ys[root.k]
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
