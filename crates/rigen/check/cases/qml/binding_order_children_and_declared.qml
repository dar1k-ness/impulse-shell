// `children: [...]` beside declared objects. QmlIR::Object::appendBinding puts a binding to an
// ordinary property before the ones written above it and a binding to the default property
// after the ones written above it, and QQmlObjectCreator applies them in that order: the
// items of `children` come before every declared item, wherever the binding is written.
// qt: children viaChildren1,viaChildren2,decl1,decl2 data obj1,obj2,viaChildren1,viaChildren2,decl1,decl2
import QtQuick
Item {
    id: root
    property string result: ""
    function names(l) {
        var s = []
        for (var i = 0; i < l.length; i++)
            s.push(l[i].objectName)
        return s.join(",")
    }
    Item { objectName: "decl1" }
    QtObject { objectName: "obj1" }
    children: [
        Item { objectName: "viaChildren1" },
        Item { objectName: "viaChildren2" }
    ]
    Item { objectName: "decl2" }
    QtObject { objectName: "obj2" }
    Component.onCompleted: result = "children " + names(children) + " data " + names(data)
}
