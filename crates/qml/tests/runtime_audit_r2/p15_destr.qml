import QtQuick
Item {
    id: r
    Component { id: c
        Item { objectName: "top"
            Component.onDestruction: console.log("destr top")
            Timer { Component.onDestruction: console.log("destr timer0") }
            property QtObject q1: QtObject { Component.onDestruction: console.log("destr q1") }
            Item { objectName: "k1"; Component.onDestruction: console.log("destr k1")
                property QtObject kq: QtObject { Component.onDestruction: console.log("destr kq") }
                Item { Component.onDestruction: console.log("destr k1a") } }
            property QtObject q2: QtObject { Component.onDestruction: console.log("destr q2") }
            Item { objectName: "k2"; Component.onDestruction: console.log("destr k2") }
            property list<QtObject> ql: [ QtObject { Component.onDestruction: console.log("destr ql0") }, QtObject { Component.onDestruction: console.log("destr ql1") } ]
            QtObject { Component.onDestruction: console.log("destr res") }
            property var v: QtObject { Component.onDestruction: console.log("destr v") }
            states: State { name: "s"; Component.onDestruction: console.log("destr state") }
            Timer { Component.onDestruction: console.log("destr timer") }
        }
    }
    Component.onCompleted: { var o = c.createObject(r); o.destroy() }
}
