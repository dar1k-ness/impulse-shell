// qt: content=-30,-70,300,500 childrenRect=10,20,210,380 origin=0,0 contentSize=300,500 contentXY=30,70 | a.parent=content a->root=5,-13 a->view=-15,-43 root->b=-10,-160 | view.childAt(50,50)=content content.childAt(150,320)=b view.childrenRect=-30,-70,300,500 | bare content=0,0,120,90 contentSize=-1,-1 childrenRect=5,6,300,20 k->root=255,36
import QtQuick
Item {
    id: root
    width: 500; height: 400
    property string result: ""
    function nm(o) { return o ? (o.objectName || "?") : "null" }
    function r(o) { return o ? [o.x, o.y, o.width, o.height].join(",") : "null" }
    function pt(p) { return p.x + "," + p.y }
    Flickable {
        id: v
        objectName: "view"
        x: 20; y: 30; width: 200; height: 200
        contentWidth: 300; contentHeight: 500
        Item { id: a; objectName: "a"; x: 10; y: 20; width: 50; height: 60 }
        Item { id: b; objectName: "b"; x: 120; y: 300; width: 100; height: 100 }
        Component.onCompleted: contentItem.objectName = "content"
    }
    // No content size set: what the content item is then.
    Flickable {
        id: bare
        x: 250; y: 30; width: 120; height: 90
        Item { id: k; objectName: "k"; x: 5; y: 6; width: 300; height: 20 }
    }
    Timer { interval: 100; running: true; onTriggered: { v.contentX = 30; v.contentY = 70 } }
    Timer {
        interval: 500; running: true
        onTriggered: {
            const c = v.contentItem
            root.result = "content=" + root.r(c) + " childrenRect=" + root.r(c.childrenRect) + " origin=" + v.originX + "," + v.originY
                + " contentSize=" + v.contentWidth + "," + v.contentHeight + " contentXY=" + v.contentX + "," + v.contentY
                + " | a.parent=" + root.nm(a.parent) + " a->root=" + root.pt(a.mapToItem(root, 5, 7)) + " a->view=" + root.pt(a.mapToItem(v, 5, 7))
                + " root->b=" + root.pt(b.mapFromItem(root, 100, 100))
                + " | view.childAt(50,50)=" + root.nm(v.childAt(50, 50)) + " content.childAt(150,320)=" + root.nm(c.childAt(150, 320))
                + " view.childrenRect=" + root.r(v.childrenRect)
                + " | bare content=" + root.r(bare.contentItem) + " contentSize=" + bare.contentWidth + "," + bare.contentHeight
                + " childrenRect=" + root.r(bare.contentItem.childrenRect) + " k->root=" + root.pt(k.mapToItem(root, 0, 0))
        }
    }
}
