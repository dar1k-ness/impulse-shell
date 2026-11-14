// qt: start: a y2040 cur 60@2400 told 60; b y0 cur 60@2400 told 0,60; g y840 cur 60@1200 told 60 | same: a y2040 cur 60@2400 told 60; b y0 cur 60@2400 told 0,60; g y840 cur 60@1200 told 60 | other: a y0 cur 0@0 told 60,0; b y0 cur 0@0 told 0,60,0; g y0 cur 0@0 told 60,0 | pick: a y0 cur 30@1200 told 60,0,30; b y0 cur 20@800 told 0,60,0,20; g y240 cur 30@600 told 60,0,30 | empty: a y0 cur -1 none told 60,0,30,-1; b y0 cur -1 none told 0,60,0,20,-1; g y0 cur -1 none told 60,0,30,-1
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    function names(n) { return Array.from({ length: n }, (_, i) => "n" + i) }
    property var rows: names(100)
    property int n: 100
    property int pick: 60
    property var steps: ["same", "other", "pick", "empty"]
    property int k: -1
    property var out: []
    function at(view) {
        const c = view.currentItem
        const y = view.contentY
        return "y" + Math.round(y) + " cur " + view.currentIndex + (c ? "@" + c.y : " none") + " told " + view.told.join(",")
    }
    component Row: Item {
        width: 100; height: 40
    }
    // Its current row is bound, its model an array.
    ListView {
        id: a
        property var told: []
        onCurrentIndexChanged: told.push(currentIndex)
        width: 100; height: 400
        model: root.rows; currentIndex: root.pick
        delegate: Row {}
    }
    // Its current row is written, its model a number.
    ListView {
        id: b
        property var told: []
        onCurrentIndexChanged: told.push(currentIndex)
        x: 100; width: 100; height: 400
        model: root.n
        delegate: Row {}
        Component.onCompleted: currentIndex = 60
    }
    GridView {
        id: g
        property var told: []
        onCurrentIndexChanged: told.push(currentIndex)
        x: 200; width: 100; height: 400
        cellWidth: 50; cellHeight: 40
        model: root.rows; currentIndex: root.pick
        delegate: Item { width: 50; height: 40 }
    }
    Timer {
        interval: 180; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + ": a " + root.at(a)
                + "; b " + root.at(b) + "; g " + root.at(g))
            root.k++
            if (root.k < root.steps.length) {
                const s = root.steps[root.k]
                // An array with the same elements is the same model.
                if (s === "same") root.rows = root.names(100)
                else if (s === "other") { root.rows = root.names(80); root.n = 50 }
                else if (s === "pick") { root.pick = 30; b.currentIndex = 20 }
                else { root.rows = []; root.n = 0 }
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
