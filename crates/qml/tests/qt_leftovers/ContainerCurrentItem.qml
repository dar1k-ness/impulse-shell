import QtQuick
import QtQuick.Controls
Item {
    id: root
    Container { id: c0; contentItem: Item {} }
    Container { id: c1; contentItem: Item {}
        Item { objectName: "a" } Item { objectName: "b" } }
    TabBar { id: tb; TabButton { text: "a" } TabButton { text: "b" } }
    SwipeView { id: sv; Item { objectName: "p0" } Item { objectName: "p1" } }
    property string cur: c1.currentItem ? c1.currentItem.objectName : "none"
    onCurChanged: console.log("cur", cur)
    Component { id: ic; Item { objectName: "new" } }
    Component.onCompleted: {
        console.log("c0", c0.currentItem);
        console.log("c1", c1.currentItem.objectName, c1.currentItem === c1.itemAt(0));
        console.log("tb", tb.currentItem.text, sv.currentItem.objectName);
        c1.currentIndex = 1;
        console.log("c1", c1.currentItem.objectName);
        c1.currentIndex = 7;
        console.log("c1", c1.currentItem);
        c1.currentIndex = -1;
        console.log("c1", c1.currentItem);
        c1.currentIndex = 0;
        c1.removeItem(c1.itemAt(0));
        console.log("c1", c1.currentItem.objectName);
        tb.currentIndex = 1; sv.currentIndex = 1;
        console.log("tb", tb.currentItem.text, sv.currentItem.objectName);
        c0.addItem(ic.createObject(null));
        console.log("c0", c0.currentItem.objectName);
        Qt.quit();
    }
}
