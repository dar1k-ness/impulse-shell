// qt: start: a none; b y0 cur 60 none; c y0 cur 0@0 | open: a y2440 cur 70@2800; b y0 cur 60 none; c y0 cur 0@0 | late: a y2440 cur 70@2800; b y0 cur 0@0; c y0 cur 0@0 | c.c50: a y2440 cur 70@2800; b y0 cur 0@0; c y1640 cur 50@2000
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property var names: Array.from({ length: 100 }, (_, n) => "n" + n)
    property bool open: false
    property var late: 0
    property var steps: ["open", "late", "c.c50"]
    property int k: -1
    property var out: []
    function at(view) {
        if (!view) return "none"
        const c = view.currentItem
        return "y" + Math.round(view.contentY) + " cur " + view.currentIndex + (c ? "@" + c.y : " none")
    }
    component Row: Item {
        width: 100; height: 40
    }
    // A dialog made later whose list gets its rows and its choice from outside, as
    // SelectionDialog does.
    component Chooser: Item {
        property alias items: list.model
        property var choice
        property alias view: list
        ListView {
            id: list
            width: 100; height: 400
            currentIndex: parent.choice !== undefined ? parent.items.indexOf(parent.choice) : -1
            delegate: Row {}
        }
    }
    Loader {
        id: a
        active: root.open
        sourceComponent: Chooser { items: root.names; choice: "n70" }
    }
    // The rows come after the view is complete.
    ListView {
        id: b
        x: 100; width: 100; height: 400
        model: root.late; currentIndex: 60
        delegate: Row {}
    }
    // A row far away, the highlight on its way for 50 ms.
    ListView {
        id: c
        x: 200; width: 100; height: 400
        model: 100; highlightMoveDuration: 50
        delegate: Row {}
    }
    Timer {
        interval: 180; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + ": a " + root.at(a.item ? a.item.view : null)
                + "; b " + root.at(b) + "; c " + root.at(c))
            root.k++
            if (root.k < root.steps.length) {
                const s = root.steps[root.k]
                if (s === "open") root.open = true
                else if (s === "late") root.late = 100
                else c.currentIndex = 50
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
