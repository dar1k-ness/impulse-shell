// qt: decl.parent=VIEW viaData.parent=CONTENT bar.parent=VIEW | view.children=[CONTENT,decl,bar] | content.children=[viaData,hdr,d0,d1,hl] | view.resources=[tmr,obj] | view.data=[tmr,obj,CONTENT,decl,bar] | flickableData=[] | flickableChildren=[viaData,hdr,d0,d1,hl]
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 300
    property string result: ""
    function nm(o) { return o ? (o.objectName || (o === v.contentItem ? "CONTENT" : o === v ? "VIEW" : "?")) : "null" }
    function names(l) { if (l === undefined) return "undefined"; let a = []; for (let i = 0; i < l.length; i++) a.push(nm(l[i])); return "[" + a.join(",") + "]" }
    function where(o) { return nm(o) + ".parent=" + nm(o.parent) }
    function d() {
        return where(decl) + " " + where(viaData) + " " + where(bar)
            + " | view.children=" + names(v.children)
            + " | content.children=" + names(v.contentItem.children)
            + " | view.resources=" + names(v.resources)
            + " | view.data=" + names(v.data)
            + " | flickableData=" + names(v.flickableData)
            + " | flickableChildren=" + names(v.flickableChildren)
    }
    ListView {
        id: v
        objectName: "VIEW"
        width: 100; height: 100
        Item { id: decl; objectName: "decl"; width: 10; height: 10 }
        Timer { id: tmr; objectName: "tmr" }
        QtObject { id: obj; objectName: "obj" }
        ScrollBar.vertical: ScrollBar { id: bar; objectName: "bar" }
        flickableData: [ Item { id: viaData; objectName: "viaData" } ]
        model: 2
        delegate: Item { objectName: "d" + index; width: 50; height: 20 }
        header: Item { objectName: "hdr"; height: 5 }
        highlight: Item { objectName: "hl" }
    }
    Component.onCompleted: result = d()
}
