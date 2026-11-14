// qt: decl.parent=CONTENT bar.parent=VIEW tmr.parent=VIEW | view.children=[CONTENT,bar] | content.children=[decl,decl2] | content.parent=VIEW | view.resources=[] | view.data=[CONTENT,bar] | flickableChildren=[decl,decl2] | flickableData=[]
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 300
    property string result: ""
    function nm(o) { return o ? (o.objectName || (o === v.contentItem ? "CONTENT" : o === v ? "VIEW" : "?")) : "null" }
    function names(l) { if (l === undefined) return "undefined"; let a = []; for (let i = 0; i < l.length; i++) a.push(nm(l[i])); return "[" + a.join(",") + "]" }
    Flickable {
        id: v
        objectName: "VIEW"
        width: 100; height: 100
        contentWidth: 100; contentHeight: 400
        Item { id: decl; objectName: "decl"; width: 10; height: 10 }
        Timer { id: tmr; objectName: "tmr" }
        Item { id: decl2; objectName: "decl2"; width: 10; height: 10 }
        ScrollBar.vertical: ScrollBar { id: bar; objectName: "bar" }
    }
    Component.onCompleted: result = "decl.parent=" + nm(decl.parent) + " bar.parent=" + nm(bar.parent) + " tmr.parent=" + nm(tmr.parent)
        + " | view.children=" + names(v.children)
        + " | content.children=" + names(v.contentItem.children)
        + " | content.parent=" + nm(v.contentItem.parent)
        + " | view.resources=" + names(v.resources)
        + " | view.data=" + names(v.data)
        + " | flickableChildren=" + names(v.flickableChildren)
        + " | flickableData=" + names(v.flickableData)
}
