// qt: decl.parent=VIEW bar.parent=VIEW d0.parent=CONTENT hdr.parent=CONTENT ftr.parent=CONTENT hl.parent=CONTENT | view.children=[CONTENT(z0),decl(z0),bar(z0)] | content.children=[hdr(z1),ftr(z1),sec_a(z2),d0(z1),d1(z1),sec_b(z2),d2(z1),hl(z0)] | content.parent=VIEW content.z=0 | view.resources=[tmr(zundefined)]
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 300
    property string result: ""
    function nm(o) { return o ? (o.objectName || (o === v.contentItem ? "CONTENT" : o === v ? "VIEW" : "?")) : "null" }
    function names(l) { if (l === undefined) return "undefined"; let a = []; for (let i = 0; i < l.length; i++) a.push(nm(l[i]) + "(z" + l[i].z + ")"); return "[" + a.join(",") + "]" }
    ListModel { id: lm; ListElement { sec: "a" } ListElement { sec: "a" } ListElement { sec: "b" } }
    ListView {
        id: v
        headerPositioning: ListView.OverlayHeader; footerPositioning: ListView.PullBackFooter
        objectName: "VIEW"
        width: 100; height: 200
        Item { id: decl; objectName: "decl"; width: 10; height: 10 }
        Timer { id: tmr; objectName: "tmr" }
        ScrollBar.vertical: ScrollBar { id: bar; objectName: "bar" }
        model: lm
        currentIndex: 1
        delegate: Item { objectName: "d" + index; width: 50; height: 20 }
        header: Item { objectName: "hdr"; height: 5 }
        footer: Item { objectName: "ftr"; height: 5 }
        highlight: Item { objectName: "hl" }
        section.property: "sec"
        section.delegate: Item { objectName: "sec_" + section; height: 4 }
    }
    Component.onCompleted: result = "decl.parent=" + nm(decl.parent) + " bar.parent=" + nm(bar.parent)
        + " d0.parent=" + nm(v.itemAtIndex(0).parent) + " hdr.parent=" + nm(v.headerItem.parent) + " ftr.parent=" + nm(v.footerItem.parent) + " hl.parent=" + nm(v.highlightItem.parent)
        + " | view.children=" + names(v.children)
        + " | content.children=" + names(v.contentItem.children)
        + " | content.parent=" + nm(v.contentItem.parent) + " content.z=" + v.contentItem.z
        + " | view.resources=" + names(v.resources)
}
