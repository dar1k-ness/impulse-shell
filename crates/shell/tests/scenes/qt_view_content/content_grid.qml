// qt: content=0,-30,200,265 childrenRect=0,-15,200,265 origin=0,-15 contentSize=-1,265 contentY=30 | d3=100,40,90,30 d3.parent=content d3->root=125,47 | view.childAt(50,50)=content content.childAt(150,50)=d3 | hdr=0,-15,200,15 ftr=0,240,200,10
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    function nm(o) { return o ? (o.objectName || "?") : "null" }
    function r(o) { return o ? [o.x, o.y, o.width, o.height].join(",") : "null" }
    function pt(p) { return p.x + "," + p.y }
    GridView {
        id: v
        objectName: "view"
        x: 20; y: 30; width: 200; height: 100
        cellWidth: 100; cellHeight: 40
        model: 12
        header: Item { objectName: "hdr"; width: 200; height: 15 }
        footer: Item { objectName: "ftr"; width: 200; height: 10 }
        delegate: Item { objectName: "d" + index; width: 90; height: 30 }
        Component.onCompleted: contentItem.objectName = "content"
    }
    Timer { interval: 100; running: true; onTriggered: v.contentY = 30 }
    Timer {
        interval: 500; running: true
        onTriggered: {
            const c = v.contentItem, d3 = v.itemAtIndex(3)
            root.result = "content=" + root.r(c) + " childrenRect=" + root.r(c.childrenRect) + " origin=" + v.originX + "," + v.originY
                + " contentSize=" + v.contentWidth + "," + v.contentHeight + " contentY=" + v.contentY
                + " | d3=" + root.r(d3) + " d3.parent=" + root.nm(d3 ? d3.parent : null) + " d3->root=" + (d3 ? root.pt(d3.mapToItem(root, 5, 7)) : "null")
                + " | view.childAt(50,50)=" + root.nm(v.childAt(50, 50)) + " content.childAt(150,50)=" + root.nm(c.childAt(150, 50))
                + " | hdr=" + root.r(v.headerItem) + " ftr=" + root.r(v.footerItem)
        }
    }
}
