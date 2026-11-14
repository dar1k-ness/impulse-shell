import QtQuick
import QtQuick.Controls
Item {
    id: root
    property list<Item> tabs: [ Item { objectName: "t1" }, Item { objectName: "t2" } ]
    property bool three: false
    Item { id: extra; objectName: "t3" }
    function names(l) { var o = []; for (var i = 0; i < l.length; i++) o.push(l[i].objectName); return o.join(",") }
    SwipeView {
        id: bound
        contentChildren: root.three ? [root.tabs[0], root.tabs[1], extra] : root.tabs
    }
    SwipeView {
        id: plain
        Item { objectName: "p1" }
        Item { objectName: "p2" }
        Item { objectName: "p3" }
    }
    SwipeView {
        id: rep
        Repeater { model: 2; Item { objectName: "r" + index } }
    }
    Container {
        id: cont
        contentItem: Row { }
        contentChildren: [ Item { objectName: "c1" } ]
    }
    TabBar {
        id: bar
        TabButton { objectName: "b1"; text: "a" }
        TabButton { objectName: "b2"; text: "b" }
    }
    Component.onCompleted: {
        console.log("bound", typeof bound.contentChildren, bound.contentChildren.length, names(bound.contentChildren), bound.count, bound.currentIndex);
        console.log("plain", plain.contentChildren.length, names(plain.contentChildren), plain.count, plain.currentIndex, plain.itemAt(1).objectName);
        console.log("rep", rep.contentChildren.length, names(rep.contentChildren), rep.count);
        console.log("cont", cont.contentChildren.length, names(cont.contentChildren), cont.count);
        console.log("bar", bar.contentChildren.length, names(bar.contentChildren), bar.count, bar.currentIndex);
        three = true;
        console.log("bound3", bound.contentChildren.length, names(bound.contentChildren), bound.count);
        three = false;
        console.log("bound2", bound.contentChildren.length, names(bound.contentChildren), bound.count);
        console.log("extra", extra.parent === null);
        plain.contentChildren = [extra];
        console.log("plain set", plain.contentChildren.length, names(plain.contentChildren), plain.count);
        console.log("END"); Qt.quit();
    }
}
