import QtQuick
Item {
    id: r
    objectName: "root"
    Item {
        id: mid
        objectName: "mid"
        QtObject { id: q; Component.onCompleted: console.log("q.parent:", q.parent, "'parent' in q:", "parent" in q) }
        Connections { id: c; Component.onCompleted: console.log("c.parent:", c.parent) }
        Binding { id: b; Component.onCompleted: console.log("b.parent:", b.parent) }
    }
}
