// qt: start: a y2040 cur 60@2400 rows 51..69@2040; b y0 cur 0@0 rows 0..18@0; g y0 cur 0@0 | b.y2000: a y2040 cur 60@2400 rows 51..69@2040; b y2000 cur 0@0 rows 0..0@0,50..68@2000; g y0 cur 0@0 | b.c0: a y2040 cur 60@2400 rows 51..69@2040; b y2000 cur 0@0 rows 0..0@0,50..68@2000; g y0 cur 0@0 | b.c1: a y2040 cur 60@2400 rows 51..69@2040; b y40 cur 1@40 rows 1..19@40; g y0 cur 0@0 | g.c190: a y2040 cur 60@2400 rows 51..69@2040; b y40 cur 1@40 rows 1..19@40; g y3440 cur 190@3800
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int choice: 60
    property var steps: ["b.y2000", "b.c0", "b.c1", "g.c190"]
    property int k: -1
    property var out: []
    // The rows that have a delegate, as runs of `index`, and where the first of each stands.
    function rows(view) {
        const l = []
        for (const c of view.contentItem.children)
            if (c.isRow && c.visible) l.push(c)
        l.sort((p, q) => p.index - q.index)
        const runs = []
        for (let n = 0; n < l.length; n++) {
            if (n && l[n].index === l[n - 1].index + 1) continue
            let m = n
            while (m + 1 < l.length && l[m + 1].index === l[m].index + 1) m++
            runs.push(l[n].index + ".." + l[m].index + "@" + l[n].y)
        }
        return runs.join(",")
    }
    function at(view) {
        const c = view.currentItem
        return "y" + Math.round(view.contentY) + " cur " + view.currentIndex + (c ? "@" + c.y : " none")
            + (view === g ? "" : " rows " + rows(view))
    }
    component Row: Item {
        width: 100; height: 40
        required property int index
        readonly property bool isRow: true
    }
    // A choice far down the list from the start, as a dialog opened on the current value.
    ListView {
        id: a
        width: 100; height: 400
        model: 100; currentIndex: root.choice
        delegate: Row {}
    }
    // Scrolled away, then the first rows made current, as a search list reset to its top.
    ListView {
        id: b
        x: 100; width: 100; height: 400
        model: 100; highlightMoveDuration: 100
        delegate: Row {}
    }
    GridView {
        id: g
        x: 200; width: 100; height: 400
        cellWidth: 50; cellHeight: 40
        model: 200; highlightMoveDuration: 50
        delegate: Row { width: 50 }
    }
    // Each step reads what the step before left, then does its own.
    Timer {
        interval: 180; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + ": a " + root.at(a) + "; b " + root.at(b) + "; g " + root.at(g))
            root.k++
            if (root.k < root.steps.length) {
                const [n, s] = root.steps[root.k].split(".")
                const view = n === "b" ? b : g
                if (s[0] === "y") view.contentY = +s.slice(1); else view.currentIndex = +s.slice(1)
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
