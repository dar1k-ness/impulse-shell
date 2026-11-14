// qt: start +19 -0 rows 0..18@0:r0 h 4000 y 0 count 100 | y2000 +19 -18 rows 0..0@0:r0,50..68@2000:r50 h 4000 y 2000 count 100 | ins0 +0 -0 rows 1..1@0:r0,51..69@2000:r50 h 4040 y 2000 count 101 | rm0 +0 -0 rows 0..0@0:r0,50..68@2000:r50 h 4000 y 2000 count 100 | ins55 +1 -1 rows 0..0@0:r0,50..68@2000:r50 h 4040 y 2000 count 101 | rm60 +1 -1 rows 0..0@0:r0,50..68@2000:r50 h 4000 y 2000 count 100 | app +0 -0 rows 0..0@0:r0,50..68@2000:r50 h 4040 y 2000 count 101 | rm2 +0 -0 rows 0..0@0:r0,49..67@2000:r50 h 4000 y 2000 count 100 | y0 +17 -19 rows 0..17@40:r0 h 4000 y 0 count 100
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int made: 0
    property int gone: 0
    property var steps: ["y2000", "ins0", "rm0", "ins55", "rm60", "app", "rm2", "y0"]
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
            runs.push(l[n].index + ".." + l[m].index + "@" + (l[n].y + ":" + l[n].name))
        }
        return runs.join(",")
    }
    ListView {
        id: v
        width: 300; height: 400
        model: ListModel { id: lm; Component.onCompleted: { for (let n = 0; n < 100; n++) append({ name: "r" + n }) } }
        delegate: Item {
            width: 300; height: 40; required property string name
            required property int index
            readonly property bool isRow: true
            Component.onCompleted: root.made++
            Component.onDestruction: root.gone++
        }
    }
    // Each step reads what the step before left, then does its own.
    Timer {
        interval: 60; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + " +" + (root.made - root.was[0]) + " -" + (root.gone - root.was[1]) + " rows " + root.rows() + " h " + v.contentHeight + " y " + v.contentY + " count " + v.count)
            root.was = [root.made, root.gone]
            root.k++
            if (root.k < root.steps.length) {
                const s = root.steps[root.k]; if (s[0] === "y") v.contentY = +s.slice(1); else if (s === "app") lm.append({ name: "new" }); else if (s.startsWith("ins")) lm.insert(+s.slice(3), { name: "in" + s.slice(3) }); else lm.remove(+s.slice(2))
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
