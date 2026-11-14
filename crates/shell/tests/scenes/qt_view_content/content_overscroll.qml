// qt: list contentY=-25 content=0,25,200,320 atYBeginning=true d0->root=20,55 view.childAt(50,10)=content view.childAt(50,30)=content content.childAt(50,-5)=null | flick contentXY=-10,-25 content=10,25,300,500 atXBeginning=true a->root=270,75 flick.childAt(5,50)=fcontent flick.childAt(30,50)=fcontent
import QtQuick
Item {
    id: root
    width: 500; height: 400
    property string result: ""
    function nm(o) { return o ? (o.objectName || "?") : "null" }
    function r(o) { return o ? [o.x, o.y, o.width, o.height].join(",") : "null" }
    function pt(p) { return p.x + "," + p.y }
    ListView {
        id: v
        objectName: "view"
        x: 20; y: 30; width: 200; height: 200
        model: 8
        delegate: Item { objectName: "d" + index; width: 150; height: 40 }
        Component.onCompleted: contentItem.objectName = "content"
    }
    Flickable {
        id: f
        objectName: "flick"
        x: 250; y: 30; width: 200; height: 200
        contentWidth: 300; contentHeight: 500
        Item { id: a; objectName: "a"; x: 10; y: 20; width: 50; height: 60 }
        Component.onCompleted: contentItem.objectName = "fcontent"
    }
    // Past the start: the content item sits below and right of the view's corner.
    Timer { interval: 100; running: true; onTriggered: { v.contentY = -25; f.contentX = -10; f.contentY = -25 } }
    Timer {
        interval: 500; running: true
        onTriggered: {
            const c = v.contentItem, d0 = v.itemAtIndex(0)
            root.result = "list contentY=" + v.contentY + " content=" + root.r(c) + " atYBeginning=" + v.atYBeginning
                + " d0->root=" + root.pt(d0.mapToItem(root, 0, 0)) + " view.childAt(50,10)=" + root.nm(v.childAt(50, 10))
                + " view.childAt(50,30)=" + root.nm(v.childAt(50, 30)) + " content.childAt(50,-5)=" + root.nm(c.childAt(50, -5))
                + " | flick contentXY=" + f.contentX + "," + f.contentY + " content=" + root.r(f.contentItem) + " atXBeginning=" + f.atXBeginning
                + " a->root=" + root.pt(a.mapToItem(root, 0, 0)) + " flick.childAt(5,50)=" + root.nm(f.childAt(5, 50))
                + " flick.childAt(30,50)=" + root.nm(f.childAt(30, 50))
        }
    }
}
