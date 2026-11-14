import QtQuick
import QtQuick.Controls
Item {
    component NoAnim: Transition { NumberAnimation { duration: 0 } }
    component P: Item {
        property bool shown: false
        StackView.onActivating: console.log(objectName, "activating")
        StackView.onActivated: console.log(objectName, "activated")
        StackView.onDeactivating: console.log(objectName, "deactivating")
        StackView.onDeactivated: console.log(objectName, "deactivated")
        StackView.onRemoved: { console.log(objectName, "removed"); destroy() }
        Component.onDestruction: console.log(objectName, "destroyed")
    }
    StackView { id: sv
        pushEnter: NoAnim {}
        pushExit: NoAnim {}
        popEnter: NoAnim {}
        popExit: NoAnim {}
        initialItem: P { objectName: "first" }
    }
    Component { id: page; P { objectName: "page" } }
    property var held
    Component.onCompleted: {
        console.log("init", sv.depth, sv.currentItem.objectName, sv.busy)
        held = sv.push(page.createObject(null, { objectName: "sub" }))
        console.log("push ->", held.objectName, sv.depth, sv.currentItem.objectName, "first visible", sv.children.length)
    }
    Timer { interval: 100; running: true; onTriggered: {
        console.log("t100 busy", sv.busy, "depth", sv.depth)
        var q = sv.pop(); console.log("pop ->", q ? q.objectName : q, sv.depth, sv.currentItem.objectName)
        console.log("pop at depth 1 ->", sv.pop(), sv.depth)
    } }
    Timer { interval: 300; running: true; onTriggered: console.log("t300 depth", sv.depth, "held", held, "children", sv.children.length) }
}
