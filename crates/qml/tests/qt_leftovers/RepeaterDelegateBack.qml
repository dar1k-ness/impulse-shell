import QtQuick
Item {
    id: root
    property bool over: false
    Component {
        id: row
        Item {
            objectName: "a" + index
            Component.onCompleted: console.log("made", objectName, "count", rep.count)
            // Qt destroys the last ones when the engine goes, after Qt.quit().
            Component.onDestruction: if (!root.over) console.log("gone", objectName, "count", rep.count)
        }
    }
    Component {
        id: other
        Item {
            objectName: "b" + index
            Component.onCompleted: console.log("made", objectName, "count", rep.count)
            Component.onDestruction: if (!root.over) console.log("gone", objectName, "count", rep.count)
        }
    }
    Item {
        id: box
        Repeater {
            id: rep
            model: 2
            delegate: row
            onItemAdded: (i, it) => console.log("added", i, it.objectName, "count", count)
            onItemRemoved: (i, it) => console.log("removed", i, it.objectName, "count", count)
            onCountChanged: console.log("count", count)
            onDelegateChanged: console.log("delegate", delegate === row ? "row" : delegate === other ? "other" : delegate)
        }
    }
    Timer {
        interval: 1; running: true
        onTriggered: {
            console.log("no delegate"); rep.delegate = null;
            console.log("delegate back"); rep.delegate = row;
            console.log("another delegate"); rep.delegate = other;
            console.log("end", rep.count);
            root.over = true;
            Qt.quit();
        }
    }
}
