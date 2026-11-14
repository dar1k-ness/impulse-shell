import QtQuick
import Rustshell
FloatingWindow { visible: true; implicitWidth: 700; implicitHeight: 500
Item {
    id: root
    width: 700; height: 500
    component Row: Rectangle { required property int index; width: 200; height: 30 }
    component Col: Rectangle { required property int index; width: 30; height: 50 }
    component Cell: Rectangle { required property int index; width: 50; height: 40 }
    ListView { id: lv; x: 0; width: 200; height: 200; model: 40; delegate: Row {} }
    ListView { id: lm; x: 0; y: 250; width: 200; height: 200; model: 40; delegate: Row {}
        topMargin: 10; bottomMargin: 20; spacing: 4
        header: Item { width: 200; height: 16 }
        footer: Item { width: 200; height: 12 } }
    ListView { id: lh; x: 210; width: 200; height: 50; model: 40; delegate: Col {}
        orientation: ListView.Horizontal; leftMargin: 5; rightMargin: 5; spacing: 4 }
    ListView { id: ls; x: 210; y: 60; width: 200; height: 200; model: 40; delegate: Row {}
        snapMode: ListView.SnapToItem }
    ListView { id: lr; x: 210; y: 270; width: 200; height: 200; model: 40; delegate: Row {}
        highlightRangeMode: ListView.ApplyRange; preferredHighlightBegin: 40; preferredHighlightEnd: 100 }
    GridView { id: gv; x: 420; width: 200; height: 100; model: 40; delegate: Cell {}
        cellWidth: 50; cellHeight: 40 }
    GridView { id: gm; x: 420; y: 110; width: 200; height: 100; model: 38; delegate: Cell {}
        cellWidth: 50; cellHeight: 40; topMargin: 10; bottomMargin: 30 }
    GridView { id: gs; x: 420; y: 220; width: 200; height: 100; model: 40; delegate: Cell {}
        cellWidth: 50; cellHeight: 40; snapMode: GridView.SnapToRow }


    // Each view starts a call from `base`, where it shows one row cut at its start, whole rows,
    // and one row cut at its end. `at`: the first row, one before the view, the cut one at the
    // start, a whole one, the cut one at the end, one after the view, the last row.
    property var views: [
        { n: "lv", v: lv, p: "contentY", base: 315, at: [0, 3, 10, 12, 17, 30, 39] },
        { n: "lm", v: lm, p: "contentY", base: 315, at: [0, 3, 9, 12, 14, 30, 39] },
        { n: "lh", v: lh, p: "contentX", base: 315, at: [0, 3, 9, 12, 15, 30, 39] },
        { n: "ls", v: ls, p: "contentY", base: 315, at: [0, 3, 10, 12, 17, 30, 39] },
        { n: "lr", v: lr, p: "contentY", base: 315, at: [0, 3, 10, 12, 17, 30, 39] },
        { n: "gv", v: gv, p: "contentY", base: 130, at: [0, 6, 13, 17, 22, 30, 39] },
        { n: "gm", v: gm, p: "contentY", base: 130, at: [0, 6, 13, 17, 22, 30, 37] },
        { n: "gs", v: gs, p: "contentY", base: 130, at: [0, 6, 13, 17, 22, 30, 39] }
    ]
    property var modes: ["Beginning", "Center", "End", "Visible", "Contain", "SnapPosition"]
    property int step: 0
    property var last: null
    Timer { interval: 32; repeat: true; running: true; onTriggered: {
        if (root.last) console.log("rest", root.last.n, root.last.v[root.last.p])
        const per = root.modes.length * 7
        if (root.step >= root.views.length * per) {
            // WallpaperSelectorContent.qml:308, from the top of the grid.
            gv.contentY = 0; gv.positionViewAtIndex(13, GridView.Contain); console.log("gv top Contain 13", gv.contentY)
            gm.contentY = -10; gm.positionViewAtIndex(13, GridView.Contain); console.log("gm top Contain 13", gm.contentY)
            // TaskViewContent.qml:257, from the start of the row.
            lh.contentX = -5; lh.positionViewAtIndex(8, ListView.Contain); console.log("lh start Contain 8", lh.contentX)
            console.log("done"); running = false; Qt.quit(); return
        }
        const e = root.views[Math.floor(root.step / per)]
        const mode = (root.step % per - root.step % 7) / 7, i = e.at[root.step % 7]
        e.v[e.p] = e.base
        e.v.positionViewAtIndex(i, mode)
        console.log(e.n, root.modes[mode], i, e.v[e.p])
        root.last = e
        root.step++
    } }
}
}
