import QtQuick
import QtQuick.Controls
Item {
    id: root
    component Anim: Transition { NumberAnimation { property: "opacity"; from: 0.5; to: 1; duration: 40 } }
    component P: Item {
        StackView.onActivating: console.log(" ", objectName, "activating", sv.depth, sv.currentItem ? sv.currentItem.objectName : null, sv.busy, visible)
        StackView.onActivated: console.log(" ", objectName, "activated", sv.depth, sv.currentItem ? sv.currentItem.objectName : null, sv.busy, visible)
        StackView.onDeactivating: console.log(" ", objectName, "deactivating", sv.depth, sv.currentItem ? sv.currentItem.objectName : null, sv.busy, visible)
        StackView.onDeactivated: console.log(" ", objectName, "deactivated", sv.depth, sv.currentItem ? sv.currentItem.objectName : null, sv.busy, visible)
        StackView.onRemoved: console.log(" ", objectName, "removed", sv.depth, sv.busy)
    }
    StackView { id: sv
        pushEnter: Anim {}
        pushExit: Anim {}
        popEnter: Anim {}
        popExit: Anim {}
        replaceEnter: Anim {}
        replaceExit: Anim {}
        onBusyChanged: { console.log("  busy", busy); if (!busy) root.next() }
        onDepthChanged: console.log("  depth", depth)
        onCurrentItemChanged: console.log("  current", currentItem ? currentItem.objectName : null)
    }
    Component { id: page; P {} }
    property int step: 0
    function mk(n) { return page.createObject(null, { objectName: n }) }
    property var pre: ({})
    function next() {
        step++
        console.log("step", step)
        if (step === 1) { var a = sv.push(mk("a"), StackView.Immediate); console.log("push a ->", a.objectName, sv.depth, sv.busy); next() }
        else if (step === 2) { var b = sv.push([mk("b"), mk("c"), page, { objectName: "d" }]); console.log("push b c d ->", b.objectName, sv.depth, sv.busy) }
        else if (step === 3) { var r = sv.replace(mk("e")); console.log("replace e ->", r.objectName, sv.depth, sv.busy) }
        else if (step === 4) { var q = sv.pop(null); console.log("pop null ->", q.objectName, sv.depth, sv.busy) }
        else if (step === 5) { var r2 = sv.replace(null, pre.g, StackView.Immediate); console.log("replace all imm ->", r2.objectName, sv.depth, sv.busy); next() }
        else if (step === 6) { sv.push(pre.h, StackView.Immediate); sv.push(pre.i, StackView.Immediate); var q2 = sv.pop(sv.get(0), StackView.Immediate); console.log("pop to item imm ->", q2.objectName, sv.depth, sv.busy); next() }
        else if (step === 7) { sv.clear(); console.log("clear", sv.depth, sv.busy, sv.currentItem); Qt.quit() }
    }
    Component.onCompleted: { pre = { g: mk("g"), h: mk("h"), i: mk("i") }; next() }
}
