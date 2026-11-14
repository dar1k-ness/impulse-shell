// `children: [...]` and `data: [...]` beside declared objects: the items of `children` first
// (a binding to an ordinary property), then the declared objects and the objects of `data` in
// the order written (bindings to the default property).
// qt: children viaChildren1,viaChildren2,decl1,viaData1,decl2 data viaResources,viaDataObj,obj1,viaChildren1,viaChildren2,decl1,viaData1,decl2
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
    data: [
        Item { objectName: "viaData1" },
        QtObject { objectName: "viaDataObj" }
    ]
    QtObject { objectName: "obj1" }
    children: [
        Item { objectName: "viaChildren1" },
        Item { objectName: "viaChildren2" }
    ]
    Item { objectName: "decl2" }
    resources: [
        QtObject { objectName: "viaResources" }
    ]
    Component.onCompleted: result = "children " + names(children) + " data " + names(data)
}
