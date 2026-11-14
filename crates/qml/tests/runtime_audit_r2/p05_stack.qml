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
        onBusyChanged: { console.log("  busy", busy); if (!busy) root.next() }
        onDepthChanged: console.log("  depth", depth)
        onCurrentItemChanged: console.log("  current", currentItem ? currentItem.objectName : null)
    }
    Component { id: page; P {} }
    property int step: 0
    function next() {
        step++
        console.log("step", step)
        if (step === 1) { var a = sv.push(page.createObject(null, { objectName: "a" })); console.log("push a ->", a.objectName, sv.depth, sv.busy); next() }
        else if (step === 2) { var b = sv.push(page.createObject(null, { objectName: "b" })); console.log("push b ->", b.objectName, sv.depth, sv.busy) }
        else if (step === 3) { var c = sv.push(page.createObject(null, { objectName: "c" }), StackView.Immediate); console.log("push c imm ->", c.objectName, sv.depth, sv.busy); next() }
        else if (step === 4) { var q = sv.pop(); console.log("pop ->", q.objectName, sv.depth, sv.busy) }
        else if (step === 5) { var q2 = sv.pop(StackView.Immediate); console.log("pop imm ->", q2.objectName, sv.depth, sv.busy); next() }
        else if (step === 6) { sv.push(page.createObject(null, { objectName: "d" })); sv.push(page.createObject(null, { objectName: "e" })); console.log("two pushes", sv.depth, sv.busy) }
        else if (step === 7) { sv.clear(); console.log("clear", sv.depth, sv.busy, sv.currentItem); Qt.quit() }
    }
    Component.onCompleted: next()
}
