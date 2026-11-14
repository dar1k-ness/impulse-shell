import QtQuick
import QtQuick.Controls
Item {
    id: root
    property var seen: []
    Container { id: a; contentItem: Item {}
        onCurrentIndexChanged: root.seen.push("a idx " + currentIndex)
        onCountChanged: root.seen.push("a count " + count)
        Item {}
        Item {}
    }
    Container { id: b; contentItem: Item {}
        currentIndex: -1
        Item {}
    }
    Container { id: c; contentItem: Item {}
        Item {}
        currentIndex: -1
    }
    Container { id: e; contentItem: Item {}
        property int want: 1
        currentIndex: want
        onCurrentIndexChanged: root.seen.push("e idx " + currentIndex)
        Item {}
        Item {}
    }
    SwipeView { id: f
        onCurrentIndexChanged: root.seen.push("f idx " + currentIndex)
        Repeater { model: 2; Item {} }
    }
    Menu { id: m
        onCurrentIndexChanged: root.seen.push("m idx " + currentIndex)
        MenuItem { text: "one" }
        MenuItem { text: "two" }
    }
    Component.onCompleted: {
        console.log(seen.join("; "));
        console.log(m.currentIndex, m.count);
        console.log(a.currentIndex, b.currentIndex, c.currentIndex, e.currentIndex, f.currentIndex, f.count);
        Qt.quit();
    }
}
