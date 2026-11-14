import QtQuick
import QtQuick.Controls
Item {
    id: root
    Container { id: c0; contentItem: Item {} }
    Container { id: c1; contentItem: Item {}
        Item {} Item {} }
    Container { id: c2; contentItem: Item {}
        currentIndex: 1
        Item {}
        Item {} }
    TabBar { id: tb0 }
    TabBar { id: tb1; TabButton { text: "a" } TabButton { text: "b" } }
    SwipeView { id: sv0 }
    SwipeView { id: sv1; Item {} Item {} }
    SwipeView { id: sv2; Repeater { model: 3; Item {} } }
    Component { id: ic; Item {} }
    Container { id: d; contentItem: Item {}
        onCurrentIndexChanged: console.log("d idx", currentIndex)
        onCountChanged: console.log("d count", count)
    }
    Component.onCompleted: {
        console.log("c0", c0.currentIndex, c0.count);
        console.log("c1", c1.currentIndex, c1.count, c1.itemAt(0) !== null);
        console.log("c2", c2.currentIndex, c2.count, c2.itemAt(0) !== null);
        console.log("tb0", tb0.currentIndex, tb0.count);
        console.log("tb1", tb1.currentIndex, tb1.count, tb1.itemAt(0).text);
        console.log("sv0", sv0.currentIndex, sv0.count);
        console.log("sv1", sv1.currentIndex, sv1.count, sv1.itemAt(0) !== null);
        console.log("sv2", sv2.currentIndex, sv2.count);
        var a = ic.createObject(null), b = ic.createObject(null), e = ic.createObject(null);
        console.log("--add"); d.addItem(a);
        console.log("d", d.currentIndex, d.count);
        console.log("--add2"); d.addItem(b);
        console.log("d", d.currentIndex, d.count);
        console.log("--insert0"); d.insertItem(0, e);
        console.log("d", d.currentIndex, d.count);
        console.log("--remove cur"); d.removeItem(a);
        console.log("d", d.currentIndex, d.count);
        console.log("--remove 0"); d.removeItem(d.itemAt(0));
        console.log("d", d.currentIndex, d.count);
        console.log("--remove last"); d.removeItem(d.itemAt(0));
        console.log("d", d.currentIndex, d.count);
        console.log("--set 5"); d.currentIndex = 5;
        console.log("d", d.currentIndex);
        var f = ic.createObject(null);
        console.log("--add at 5"); d.addItem(f);
        console.log("d", d.currentIndex, d.count);
        console.log("--set -1"); d.currentIndex = -1;
        console.log("d", d.currentIndex);
        var g = ic.createObject(null);
        console.log("--add at -1"); d.addItem(g);
        console.log("d", d.currentIndex, d.count);
        console.log("--take"); var t = d.takeItem(0);
        console.log("d", d.currentIndex, d.count);
        console.log("--move"); d.addItem(t); d.currentIndex = 0; d.moveItem(0, 1);
        console.log("d", d.currentIndex, d.count);
        console.log("--dec"); d.currentIndex = 0; d.decrementCurrentIndex();
        console.log("d", d.currentIndex);
        console.log("--inc"); d.incrementCurrentIndex(); d.incrementCurrentIndex(); d.incrementCurrentIndex();
        console.log("d", d.currentIndex);
        Qt.quit();
    }
}
