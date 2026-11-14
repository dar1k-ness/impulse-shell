import QtQuick
Item {
    id: r
    Item { id: a; objectName: "a"; onChildrenChanged: console.log("a.childrenChanged", children.length)
        Item { id: a1; objectName: "a1"; onParentChanged: console.log("a1.parentChanged", parent ? parent.objectName : parent) }
        Item { id: a2; objectName: "a2" }
    }
    Item { id: b; objectName: "b"; onChildrenChanged: console.log("b.childrenChanged", children.length) }
    QtObject { id: q }
    Component.onCompleted: {
        a1.parent = b; console.log("moved", a.children.length, b.children.length, a1.parent.objectName)
        a1.parent = b; console.log("same parent again")
        b.children = [a2]; console.log("children=", a.children.length, b.children.length, a1.parent ? a1.parent.objectName : a1.parent, a2.parent.objectName)
        b.children = []; console.log("children=[]", b.children.length, a2.parent ? a2.parent.objectName : a2.parent)
        a.data = [a1, q]; console.log("data=", a.children.length, a.resources.length, a.data.length)
        try { a.parent = a1; console.log("cycle allowed", a.parent.objectName) } catch (e) { console.log("cycle err", e.message) }
        b.children.push(a2); console.log("push", b.children.length)
        console.log("childAt", r.childAt(0, 0), a.childrenRect.width)
    }
}
