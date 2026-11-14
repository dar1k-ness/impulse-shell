import QtQuick
import QtQuick.Controls
Item {
    id: root
    property int n: 2
    property bool over: false
    property var names: ["x", "y"]
    function items(c) {
        var s = [];
        for (var i = 0; i < c.count; i++)
            s.push(c.itemAt(i).objectName);
        return s.join(",") + " @" + c.currentIndex;
    }
    // A number model: every change makes all delegates again.
    SwipeView {
        id: sv
        onCountChanged: console.log("sv count", count, "idx", currentIndex, currentItem ? currentItem.objectName : null)
        onCurrentIndexChanged: console.log("sv idx", currentIndex, "count", count)
        onCurrentItemChanged: console.log("sv item", currentItem ? currentItem.objectName : null)
        Repeater {
            id: rep
            model: root.n
            Item {
                objectName: "p" + index
                Component.onCompleted: console.log("made", objectName)
                // Qt destroys the last ones when the engine goes, after Qt.quit().
                Component.onDestruction: if (!root.over) console.log("gone", objectName)
            }
            onItemAdded: (i, it) => console.log("rep added", i, it.objectName, "sv", sv.count, sv.currentIndex)
            onItemRemoved: (i, it) => console.log("rep removed", i, it.objectName, "sv", sv.count, sv.currentIndex)
            onCountChanged: console.log("rep count", count)
        }
    }
    // Rows of a ListModel come and go one by one, between two declared items.
    ListModel { id: lm }
    SwipeView {
        id: lv
        currentIndex: 1
        onCountChanged: console.log("lv count", count, "idx", currentIndex, currentItem ? currentItem.objectName : null)
        onCurrentIndexChanged: console.log("lv idx", currentIndex, "count", count)
        onCurrentItemChanged: console.log("lv item", currentItem ? currentItem.objectName : null)
        Item { objectName: "first" }
        Repeater {
            model: lm
            Item {
                required property string name
                objectName: "r-" + name
                Component.onCompleted: console.log("made", objectName)
                Component.onDestruction: console.log("gone", objectName)
            }
            onItemAdded: (i, it) => console.log("lrep added", i, it.objectName, "lv", lv.count, lv.currentIndex)
            onItemRemoved: (i, it) => console.log("lrep removed", i, it.objectName, "lv", lv.count, lv.currentIndex)
            onCountChanged: console.log("lrep count", count)
        }
        Item { objectName: "last" }
    }
    // An array model after a declared item, and an item made by createObject.
    Component { id: page; Item { objectName: "made-later" } }
    SwipeView {
        id: av
        onCountChanged: console.log("av count", count, "idx", currentIndex, currentItem ? currentItem.objectName : null)
        onCurrentIndexChanged: console.log("av idx", currentIndex, "count", count)
        onCurrentItemChanged: console.log("av item", currentItem ? currentItem.objectName : null)
        Item { objectName: "head" }
        Repeater {
            model: root.names
            Item { objectName: "a-" + modelData }
        }
    }
    Timer {
        interval: 1; running: true
        onTriggered: {
            console.log("start", root.items(sv), "|", root.items(lv), "|", root.items(av));
            console.log("n = 4"); root.n = 4; console.log(root.items(sv));
            sv.currentIndex = 3;
            console.log("n = 1"); root.n = 1; console.log(root.items(sv));
            console.log("n = 0"); root.n = 0; console.log(root.items(sv));
            console.log("n = 3"); root.n = 3; console.log(root.items(sv));

            console.log("append"); lm.append({name: "a"}); lm.append({name: "b"}); console.log(root.items(lv));
            console.log("insert"); lm.insert(0, {name: "z"}); console.log(root.items(lv));
            lv.currentIndex = 2;
            console.log("insert before current"); lm.insert(1, {name: "y"}); console.log(root.items(lv));
            console.log("remove before current"); lm.remove(0); console.log(root.items(lv));
            console.log("remove current"); lm.remove(1); console.log(root.items(lv));
            console.log("move"); lm.move(0, 1, 1); console.log(root.items(lv));
            console.log("clear"); lm.clear(); console.log(root.items(lv));

            av.currentIndex = 2;
            console.log("names"); root.names = ["q"]; console.log(root.items(av));
            console.log("createObject"); page.createObject(av); console.log(root.items(av));
            console.log("no names"); root.names = []; console.log(root.items(av));
            root.over = true;
            Qt.quit();
        }
    }
    Component.onCompleted: console.log("completed", root.items(sv), "|", root.items(lv), "|", root.items(av))
}
