// A property an Item declares in QML under the name of one of QQuickItem's lists (`data`,
// `resources`) hides that list: it holds what is assigned to it, the item's children are not in it,
// and a list written to it reparents nothing.
// qt: a {"t":21} 0; b 2 [1,{"k":2}] 0; c 1 own 1; d 3 [1,2,3] 1 [7]; e 1 child 2 1
import QtQuick
Item {
    id: root
    property string result: ""
    Item {
        id: a
        property var data: ({ "t": 21 })
    }
    Item {
        id: b
        property list<var> data: [1, { "k": 2 }]
    }
    Item {
        id: c
        property list<QtObject> resources: [QtObject { objectName: "own" }]
        QtObject { objectName: "child" }
        Item {}
    }
    Item {
        id: d
        property list<var> resources: [1, 2]
    }
    // Not declared: QQuickItem's own lists.
    Item {
        id: e
        QtObject { objectName: "child" }
        Item {}
    }
    Component.onCompleted: {
        result += "a " + JSON.stringify(a.data) + " " + a.children.length + "; "
        result += "b " + b.data.length + " " + JSON.stringify(b.data) + " " + b.children.length + "; "
        result += "c " + c.resources.length + " " + c.resources[0].objectName + " " + c.children.length + "; "
        d.resources.push(3)
        result += "d " + d.resources.length + " " + JSON.stringify(d.resources)
        d.resources = [7]
        result += " " + d.resources.length + " " + JSON.stringify(d.resources) + "; "
        result += "e " + e.resources.length + " " + e.resources[0].objectName + " " + e.data.length + " " + e.children.length
    }
}
