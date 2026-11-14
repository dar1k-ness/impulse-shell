import QtQuick
import QtQuick.Controls
Item {
    id: root
    property bool over: false
    property var names: ["x", "y", "z"]
    function items(c) {
        var s = [];
        for (var i = 0; i < c.count; i++)
            s.push(c.itemAt(i).objectName);
        return s.join(",") + " @" + c.currentIndex;
    }
    Component {
        id: row
        Item {
            objectName: "d-" + modelData
            Component.onCompleted: console.log("made", objectName, "sv", sv.count, sv.currentIndex)
            // Qt destroys the last ones when the engine goes, after Qt.quit().
            Component.onDestruction: if (!root.over) console.log("gone", objectName, "sv", sv.count, sv.currentIndex)
        }
    }
    // The Repeater's model is replaced, then its delegate is taken away.
    SwipeView {
        id: sv
        onCountChanged: console.log("sv count", count, "idx", currentIndex, currentItem ? currentItem.objectName : null)
        onCurrentIndexChanged: console.log("sv idx", currentIndex, "count", count)
        onCurrentItemChanged: console.log("sv item", currentItem ? currentItem.objectName : null)
        Item { objectName: "head" }
        Repeater {
            id: rep
            model: root.names
            delegate: row
            onItemAdded: (i, it) => console.log("rep added", i, it.objectName, "sv", sv.count, sv.currentIndex)
            onItemRemoved: (i, it) => console.log("rep removed", i, it.objectName, "sv", sv.count, sv.currentIndex)
            onCountChanged: console.log("rep count", count, "sv", sv.count)
        }
        Item { objectName: "tail" }
    }
    // An object made by createObject: its initial properties and its handlers see the count.
    Component {
        id: page
        Item {
            property int seen: -1
            onSeenChanged: console.log("page seen", seen, "av", av.count, av.currentIndex)
            Component.onCompleted: console.log("page made", objectName, "av", av.count, av.currentIndex)
        }
    }
    SwipeView {
        id: av
        onCountChanged: console.log("av count", count, "idx", currentIndex, currentItem ? currentItem.objectName : null)
        onCurrentIndexChanged: console.log("av idx", currentIndex, "count", count)
    }
    Timer {
        interval: 1; running: true
        onTriggered: {
            console.log("start", root.items(sv));
            sv.currentIndex = 3;
            console.log("names"); root.names = ["q", "r"]; console.log(root.items(sv));
            sv.currentIndex = 2;
            console.log("no delegate"); rep.delegate = null; console.log(root.items(sv));

            console.log("createObject");
            page.createObject(av, {objectName: "p0", seen: 1});
            page.createObject(av, {objectName: "p1", seen: 2});
            console.log(root.items(av));
            root.over = true;
            Qt.quit();
        }
    }
    Component.onCompleted: console.log("completed", root.items(sv))
}
