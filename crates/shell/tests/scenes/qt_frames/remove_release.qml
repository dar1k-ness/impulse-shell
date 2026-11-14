// A delegate running its `remove` transition is released at the view's next forced layout:
// frame 2 removes the last delegate of each list, frame 6 changes one thing, and each frame
// logs which lists still show their leaving delegate. (Qt deletes a delegate whose transition
// ended one frame later, at the next polish; it is at opacity 0 by then.)
import QtQuick
import QtQuick.Layouts
Item {
    id: scene
    width: 1540; height: 120
    property int frames: 48
    property string log: ""
    Component {
        id: list
        ListView {
            id: lv
            // The delegate that is removed, and whether it was destroyed.
            property Item last: null
            property bool gone: false
            // The first model element kept.
            property int from: 1
            width: 60; height: 69
            spacing: 3
            interactive: false
            Layout.fillWidth: true
            model: ListModel { ListElement { n: 1 } ListElement { n: 2 } ListElement { n: 3 } }
            Component.onCompleted: if (from > 1) model.remove(0, from - 1)
            delegate: Rectangle {
                required property int n
                width: 60; height: 21
                Component.onCompleted: if (n === 3) lv.last = this
                Component.onDestruction: if (n === 3) lv.gone = true
            }
            remove: Transition {
                NumberAnimation { property: "x"; to: 60; duration: 320 }
                NumberAnimation { property: "opacity"; to: 0; duration: 320 }
            }
        }
    }
    Item { id: holder; x: 1050; width: 60; height: 69 }
    ColumnLayout { id: column; x: 1120; width: 60 }
    ColumnLayout { id: columnOfOne; x: 1190; width: 60 }
    property var lists: ({})
    Component.onCompleted: {
        const names = ["none", "h", "w", "ypos", "xpos", "siblingH", "siblingW", "gap", "scroll",
            "insert", "siblingX", "siblingOpacity", "leavingH", "viewOpacity", "scrollX"]
        const made = {}
        for (let i = 0; i < names.length; i++)
            made[names[i]] = list.createObject(scene, { x: 70 * i })
        made.parentMoves = list.createObject(holder)
        // In a layout the list is as tall as its content.
        made.fitted = list.createObject(column)
        made.fittedOnly = list.createObject(columnOfOne, { from: 3 })
        for (const l of [made.fitted, made.fittedOnly])
            l.implicitHeight = Qt.binding(() => l.contentHeight)
        // Lists left empty by the removal.
        made.onlyGap = list.createObject(scene, { x: 1260, from: 3 })
        made.onlyH = list.createObject(scene, { x: 1330, from: 3 })
        // Resized by the handler that removes, after the removal and before it.
        made.hAfter = list.createObject(scene, { x: 1400 })
        made.hBefore = list.createObject(scene, { x: 1470 })
        lists = made
    }
    function sample(frame) {
        const l = lists
        if (frame === 2) {
            l.hBefore.height = 60
            for (const k in l)
                l[k].model.remove(l[k].count - 1)
            l.hAfter.height = 60
        }
        if (frame === 6) {
            l.h.height = 60
            l.w.width = 50
            l.ypos.y = 4
            l.xpos.x += 4
            holder.y = 4
            l.siblingH.itemAtIndex(0).height = 30
            l.siblingW.itemAtIndex(0).width = 30
            l.gap.spacing = 5
            l.scroll.contentY = 5
            l.insert.model.insert(0, { n: 0 })
            l.siblingX.itemAtIndex(0).x = 5
            l.siblingOpacity.itemAtIndex(0).opacity = 0.5
            l.leavingH.last.height = 30
            l.viewOpacity.opacity = 0.5
            l.scrollX.contentX = 5
            l.onlyGap.spacing = 5
            l.onlyH.height = 60
        }
        let s = frame + ":"
        for (const k in l)
            if (frame >= 2 && !l[k].gone && l[k].last.opacity > 0)
                s += " " + k
        log += s + "\n"
    }
}
