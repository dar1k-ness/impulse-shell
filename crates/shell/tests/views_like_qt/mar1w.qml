import QtQuick
import Rustshell
FloatingWindow { visible: true; implicitWidth: 700; implicitHeight: 500
Item {
    id: root
    width: 700; height: 500
    component Row: Rectangle { required property int index; width: 200; height: 30 }
    component Col: Rectangle { required property int index; width: 30; height: 50 }
    component Cell: Rectangle { required property int index; width: 50; height: 40 }
    ListView { id: lt; width: 200; height: 200; model: 40; delegate: Row {}
        topMargin: 10; bottomMargin: 20 }
    ListView { id: lh; x: 210; width: 200; height: 50; model: 40; delegate: Col {}
        orientation: ListView.Horizontal; leftMargin: 5; rightMargin: 7 }
    ListView { id: le; x: 210; y: 270; width: 200; height: 200; model: 3; delegate: Row {}
        topMargin: 10; bottomMargin: 20; header: Item { width: 200; height: 16 } }
    GridView { id: gt; x: 420; width: 200; height: 100; model: 38; delegate: Cell {}
        cellWidth: 50; cellHeight: 40; topMargin: 10; bottomMargin: 30 }
    Flickable { id: fl; x: 420; y: 110; width: 200; height: 100; contentWidth: 200; contentHeight: 400
        topMargin: 10; bottomMargin: 30; Rectangle { id: fr; width: 200; height: 400 } }
    function first(f) {
        for (let k = 0; k < f.count; k++) {
            const it = f.itemAtIndex(k)
            if (!it) continue
            const p = it.mapToItem(f, 0, 0)
            if (p.x + it.width > 0 && p.y + it.height > 0) return it
        }
        return null
    }
    // `moved`: a ListView that just moved has its rows in place after its next layout here.
    function say(tag, moved) {
        const v = (n, f, p, e, item) => console.log(tag, n, f[p], f[e], p === "contentY" ? f.contentHeight : f.contentWidth,
            f.atYBeginning, f.atYEnd, f.atXBeginning, f.atXEnd,
            moved ? "" : item ? (item.index ?? "r") + "@" + item.mapToItem(f, 0, 0).x + "," + item.mapToItem(f, 0, 0).y : "-")
        v("lt", lt, "contentY", "originY", first(lt))
        v("lh", lh, "contentX", "originX", first(lh))
        v("le", le, "contentY", "originY", first(le))
        v("gt", gt, "contentY", "originY", first(gt))
        v("fl", fl, "contentY", "originY", fr)
    }
    property int step: 0
    property var steps: [
        () => say("start"),
        () => { lt.topMargin = 30; lh.leftMargin = 15; le.topMargin = 30; gt.topMargin = 30; fl.topMargin = 30; say("margin 30") },
        () => say("margin 30 later"),
        () => { lt.contentY = 100; lh.contentX = 100; le.contentY = 100; gt.contentY = 100; fl.contentY = 100; say("at 100", true) },
        () => say("at 100 later"),
        () => { lt.topMargin = 0; lh.leftMargin = 0; le.topMargin = 0; gt.topMargin = 0; fl.topMargin = 0; say("margin 0") },
        () => { lt.positionViewAtBeginning(); lh.positionViewAtBeginning(); le.positionViewAtBeginning(); gt.positionViewAtBeginning(); say("beginning") },
        () => { lt.topMargin = 10; lh.leftMargin = 5; le.topMargin = 10; gt.topMargin = 10; fl.topMargin = 10; say("margin 10") },
        () => { lt.positionViewAtBeginning(); lh.positionViewAtBeginning(); le.positionViewAtBeginning(); gt.positionViewAtBeginning(); say("beginning 10") },
        () => { lt.positionViewAtEnd(); lh.positionViewAtEnd(); le.positionViewAtEnd(); gt.positionViewAtEnd(); say("end", true) },
        () => say("end later")
    ]
    Timer { interval: 200; repeat: true; running: true; onTriggered: {
        if (root.step >= root.steps.length) { console.log("done"); running = false; Qt.quit(); return }
        root.steps[root.step++]()
    } }
}
}
