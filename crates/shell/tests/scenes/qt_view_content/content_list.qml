// qt: content=0,-70,200,345 childrenRect=0,-15,200,345 origin=0,-15 contentSize=-1,345 contentY=70 | d3=0,120,150,40 d3.parent=content d3->root=25,87 d3->view=5,57 root->d3=80,20 | view.childAt(50,50)=content content.childAt(50,150)=d3 root.childAt(60,90)=view | hdr=0,-15,200,15 ftr=0,320,200,10
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    function nm(o) { return o ? (o.objectName || "?") : "null" }
    function r(o) { return o ? [o.x, o.y, o.width, o.height].join(",") : "null" }
    function pt(p) { return p.x + "," + p.y }
    ListView {
        id: v
        objectName: "view"
        x: 20; y: 30; width: 200; height: 200
        model: 8
        header: Item { objectName: "hdr"; width: 200; height: 15 }
        footer: Item { objectName: "ftr"; width: 200; height: 10 }
        delegate: Item { objectName: "d" + index; width: 150; height: 40 }
        Component.onCompleted: contentItem.objectName = "content"
    }
    Timer { interval: 100; running: true; onTriggered: v.contentY = 70 }
    Timer {
        interval: 500; running: true
        onTriggered: {
            const c = v.contentItem, d3 = v.itemAtIndex(3)
            root.result = "content=" + root.r(c) + " childrenRect=" + root.r(c.childrenRect) + " origin=" + v.originX + "," + v.originY
                + " contentSize=" + v.contentWidth + "," + v.contentHeight + " contentY=" + v.contentY
                + " | d3=" + root.r(d3) + " d3.parent=" + root.nm(d3.parent) + " d3->root=" + root.pt(d3.mapToItem(root, 5, 7))
                + " d3->view=" + root.pt(d3.mapToItem(v, 5, 7)) + " root->d3=" + root.pt(d3.mapFromItem(root, 100, 100))
                + " | view.childAt(50,50)=" + root.nm(v.childAt(50, 50)) + " content.childAt(50,150)=" + root.nm(c.childAt(50, 150))
                + " root.childAt(60,90)=" + root.nm(root.childAt(60, 90))
                + " | hdr=" + root.r(v.headerItem) + " ftr=" + root.r(v.footerItem)
        }
    }
}
