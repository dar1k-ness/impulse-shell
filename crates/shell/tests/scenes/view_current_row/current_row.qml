// qt: start: a 70@360 at 2440; b 0@10; h 0@0 at 0; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | b.c5: a 70@360 at 2440; b 5@220; h 0@0 at 0; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | b.c12: a 70@360 at 2440; b 12@360; h 0@0 at 0; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | b.c13: a 70@360 at 2440; b 13@360; h 0@0 at 0; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | b.c3: a 70@360 at 2440; b 3@0; h 0@0 at 0; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | b.c0: a 70@360 at 2440; b 0@10; h 0@0 at 0; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | a.y0: a 70@2800 at 0; b 0@10; h 0@0 at 0; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | a.c71: a 71@360 at 2480; b 0@10; h 0@0 at 0; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | h.c30: a 71@360 at 2480; b 0@10; h 30@150 at 1350; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | h.c2: a 71@360 at 2480; b 0@10; h 2@0 at 100; g 0@0 at 0; d 5@230 at -30; r 0@360 at -400; l none | g.c190: a 71@360 at 2480; b 0@10; h 2@0 at 100; g 190@260 at 3540; d 5@230 at -30; r 0@360 at -400; l none | g.c7: a 71@360 at 2480; b 0@10; h 2@0 at 100; g 7@0 at 120; d 5@230 at -30; r 0@360 at -400; l none | d.c29: a 71@360 at 2480; b 0@10; h 2@0 at 100; g 7@0 at 120; d 29@340 at 820; r 0@360 at -400; l none | d.c0: a 71@360 at 2480; b 0@10; h 2@0 at 100; g 7@0 at 120; d 0@30 at -30; r 0@360 at -400; l none | r.c20: a 71@360 at 2480; b 0@10; h 2@0 at 100; g 7@0 at 120; d 0@30 at -30; r 20@0 at -840; l none | r.c1: a 71@360 at 2480; b 0@10; h 2@0 at 100; g 7@0 at 120; d 0@30 at -30; r 1@360 at -440; l none | l.open: a 71@360 at 2480; b 0@10; h 2@0 at 100; g 7@0 at 120; d 0@30 at -30; r 1@360 at -440; l 70@360 at 2440
import QtQuick
Item {
    id: root
    width: 600; height: 400
    property string result: ""
    property int choice: 70
    property var steps: ["b.c5", "b.c12", "b.c13", "b.c3", "b.c0", "a.y0", "a.c71", "h.c30", "h.c2",
        "g.c190", "g.c7", "d.c29", "d.c0", "r.c20", "r.c1", "l.open"]
    property int k: -1
    property var out: []
    // The current row and where it stands in the view (in sight from 0 to the view's size),
    // then where the view is scrolled to.
    function at(view, scrolled) {
        if (!view) return "none"
        const c = view.currentItem
        const p = c ? c.mapToItem(view, 0, 0) : null
        const across = view.orientation === ListView.Horizontal
        return view.currentIndex + "@" + (p ? Math.round(across ? p.x : p.y) : "none")
            + (scrolled ? " at " + Math.round(across ? view.contentX : view.contentY) : "")
    }
    component Row: Item {
        width: 100; height: 40
        required property int index
    }
    // Opened on a choice far down the list, as a dialog on the current value.
    ListView {
        id: a
        width: 100; height: 400
        model: 100; currentIndex: root.choice; highlightMoveDuration: 100
        delegate: Row {}
    }
    // The current row goes down past the end and back, as a search list under the arrow keys.
    ListView {
        id: b
        x: 100; width: 100; height: 400
        model: 100; currentIndex: 0; highlightMoveDuration: 100
        spacing: 2; topMargin: 10; bottomMargin: 10
        delegate: Row {}
    }
    ListView {
        id: h
        x: 200; width: 200; height: 40
        orientation: ListView.Horizontal
        model: 50; currentIndex: 0; highlightMoveDuration: 100
        delegate: Row { width: 50 }
    }
    GridView {
        id: g
        x: 200; y: 50; width: 100; height: 300
        cellWidth: 50; cellHeight: 40
        model: 200; currentIndex: 0; highlightMoveDuration: 50
        delegate: Row { width: 50 }
    }
    // The last row brings the footer in sight, the first one the header.
    ListView {
        id: d
        x: 300; width: 100; height: 400
        model: 30; currentIndex: 5; highlightMoveDuration: 100
        header: Item { width: 100; height: 30 }
        footer: Item { width: 100; height: 20 }
        delegate: Row {}
    }
    ListView {
        id: r
        x: 400; width: 100; height: 400
        verticalLayoutDirection: ListView.BottomToTop
        model: 100; currentIndex: 0; highlightMoveDuration: 100
        delegate: Row {}
    }
    // A list made later with its choice already set.
    Loader {
        id: l
        x: 500; width: 100; height: 400
        active: false
        sourceComponent: ListView {
            model: 100; currentIndex: root.choice
            delegate: Row {}
        }
    }
    // Each step reads what the step before left, then does its own.
    Timer {
        interval: 400; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + ": a " + root.at(a, true)
                + "; b " + root.at(b, false) + "; h " + root.at(h, true) + "; g " + root.at(g, true)
                + "; d " + root.at(d, true) + "; r " + root.at(r, true) + "; l " + root.at(l.item, true))
            root.k++
            if (root.k < root.steps.length) {
                const [n, s] = root.steps[root.k].split(".")
                const view = {a: a, b: b, h: h, g: g, d: d, r: r}[n]
                if (s === "open") l.active = true
                else if (s[0] === "y") view.contentY = +s.slice(1)
                else view.currentIndex = +s.slice(1)
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
