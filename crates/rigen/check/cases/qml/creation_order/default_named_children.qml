// The children an item ends with when the default property is also bound by name: the
// named list stands where it is written among the declared children, whatever else the
// object binds; `children: [...]` comes before all of them.
// qt: decl,x,y,bar | decl,x,bar | decl,x,bar | decl,x,bar | decl,x,bar | decl,x,bar | decl,x,bar | k,d1,x,d2,d3
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string result: ""
    function names(l) {
        var s = []
        for (var i = 0; i < l.length; i++)
            s.push(l[i].objectName)
        return s.join(",")
    }
    component Box: Item {
        default property alias content: inner.data
        property alias inner: inner
        Item { id: inner }
    }
    Item {
        id: list
        Item { objectName: "decl" }
        data: [ Item { objectName: "x" }, Item { objectName: "y" } ]
        Item { objectName: "bar" }
    }
    Item {
        id: valueAfter
        Item { objectName: "decl" }
        data: [ Item { objectName: "x" } ]
        Item { objectName: "bar" }
        width: 5
        onWidthChanged: {}
    }
    Item {
        id: valueBefore
        width: 5
        Item { objectName: "decl" }
        data: [ Item { objectName: "x" } ]
        Item { objectName: "bar" }
    }
    Item {
        id: single
        Item { objectName: "decl" }
        data: Item { objectName: "x" }
        Item { objectName: "bar" }
    }
    Box {
        id: alias
        Item { objectName: "decl" }
        content: [ Item { objectName: "x" } ]
        Item { objectName: "bar" }
        width: 5
    }
    Column {
        id: column
        Item { objectName: "decl" }
        data: [ Item { objectName: "x" } ]
        Item { objectName: "bar" }
        anchors.margins: 1
    }
    Pane {
        id: pane
        Item { objectName: "decl" }
        contentData: [ Item { objectName: "x" } ]
        Item { objectName: "bar" }
    }
    Item {
        id: withChildren
        Item { objectName: "d1" }
        data: [ Item { objectName: "x" } ]
        Item { objectName: "d2" }
        children: [ Item { objectName: "k" } ]
        Item { objectName: "d3" }
    }
    Component.onCompleted: result = [names(list.children), names(valueAfter.children),
        names(valueBefore.children), names(single.children), names(alias.inner.children),
        names(column.children), names(pane.contentChildren), names(withChildren.children)].join(" | ")
}
