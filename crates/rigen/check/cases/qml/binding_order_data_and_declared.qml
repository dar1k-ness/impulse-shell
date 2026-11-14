// `data: [...]` beside declared objects. `data` is the default property of an Item, so
// QQmlDefaultPropertyMerger keeps the binding among the declared objects, in the order
// written.
// qt: children decl1,viaData1,viaData2,decl2 data obj1,viaDataObj,obj2,decl1,viaData1,viaData2,decl2
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
    data: [
        Item { objectName: "viaData1" },
        QtObject { objectName: "viaDataObj" },
        Item { objectName: "viaData2" }
    ]
    Item { objectName: "decl2" }
    QtObject { objectName: "obj2" }
    Component.onCompleted: result = "children " + names(children) + " data " + names(data)
}
