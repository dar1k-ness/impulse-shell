import QtQuick
Item {
    id: root
    width: 200; height: 400
    property var arr: []
    ListModel { id: lm }
    component D: Rectangle { required property int index; width: 50; height: 20; objectName: "d" + index }
    ListView {
        id: a; width: 50; height: 100; model: root.arr; delegate: D {}
        onCurrentIndexChanged: console.log("a cur", currentIndex, count, currentItem ? currentItem.objectName : null)
        onCurrentItemChanged: console.log("a item", currentIndex, currentItem ? currentItem.objectName : null)
        onCountChanged: console.log("a count", count, currentIndex)
    }
    ListView {
        id: b; width: 50; height: 100; model: lm; delegate: D {}
        onCurrentIndexChanged: console.log("b cur", currentIndex, count, currentItem ? currentItem.objectName : null)
        onCurrentItemChanged: console.log("b item", currentIndex, currentItem ? currentItem.objectName : null)
        onCountChanged: console.log("b count", count, currentIndex)
    }
    ListView {
        id: c; width: 50; height: 100; model: 3; delegate: D {}
        onCurrentIndexChanged: console.log("c cur", currentIndex, count, currentItem ? currentItem.objectName : null)
        onCurrentItemChanged: console.log("c item", currentIndex, currentItem ? currentItem.objectName : null)
    }
    ListView {
        id: d; width: 50; height: 100; model: 3; delegate: D {}
        currentIndex: -1
        onCurrentIndexChanged: console.log("d cur", currentIndex, count)
    }
    ListView {
        id: e; width: 50; height: 100; model: 5; delegate: D {}
        currentIndex: 2
        onCurrentIndexChanged: console.log("e cur", currentIndex, count)
        onCurrentItemChanged: console.log("e item", currentIndex, currentItem ? currentItem.objectName : null)
    }
    GridView {
        id: g; width: 100; height: 100; cellWidth: 50; cellHeight: 20; model: root.arr; delegate: D {}
        onCurrentIndexChanged: console.log("g cur", currentIndex, count)
    }
    ListView {
        id: h; width: 50; height: 100; delegate: D {}
        onCurrentIndexChanged: console.log("h cur", currentIndex, count)
    }
    Component.onCompleted: console.log("completed", a.currentIndex, b.currentIndex, c.currentIndex, d.currentIndex, e.currentIndex, g.currentIndex, h.currentIndex)
    Timer {
        interval: 100; running: true
        onTriggered: {
            console.log("-- arr 3"); root.arr = ["x", "y", "z"]
            console.log("-- a cur 2"); a.currentIndex = 2
            console.log("-- arr 2"); root.arr = ["x", "y"]
            console.log("-- arr 0"); root.arr = []
            console.log("-- arr 1"); root.arr = ["q"]
            console.log("-- lm append"); lm.append({n: 1}); lm.append({n: 2}); lm.append({n: 3})
            console.log("-- b cur 1"); b.currentIndex = 1
            console.log("-- lm insert 0"); lm.insert(0, {n: 0})
            console.log("-- lm insert 3"); lm.insert(3, {n: 9})
            console.log("-- lm remove 0"); lm.remove(0)
            console.log("-- lm remove cur"); lm.remove(b.currentIndex)
            console.log("-- lm remove last cur"); b.currentIndex = b.count - 1; lm.remove(b.count - 1)
            console.log("-- lm move"); b.currentIndex = 0; lm.move(0, 2, 1)
            console.log("-- lm clear"); lm.clear()
            console.log("-- lm append"); lm.append({n: 1})
            console.log("-- d model"); d.model = 4
            console.log("-- e model"); e.model = 2
            console.log("-- h model"); h.model = 2
            console.log("-- c cur 7"); c.currentIndex = 7
            console.log("-- c cur -1"); c.currentIndex = -1
            console.log("-- c model 0"); c.model = 0
            console.log("-- c model 2"); c.model = 2
            console.log("end", a.currentIndex, b.currentIndex, c.currentIndex, d.currentIndex, e.currentIndex, g.currentIndex, h.currentIndex)
        }
    }
}
