import QtQuick
import QtQuick.Controls
Item {
    id: root
    property var seen: []
    property int minus: -1
    Container { id: b; contentItem: Item {}
        currentIndex: -1
        onCurrentIndexChanged: root.seen.push("b idx " + currentIndex)
        Item {}
    }
    Container { id: c; contentItem: Item {}
        Item {}
        currentIndex: root.minus
        onCurrentIndexChanged: root.seen.push("c idx " + currentIndex)
    }
    Container { id: g; contentItem: Item {}
        property int shown: currentIndex
        onShownChanged: root.seen.push("g shown " + shown)
        Item {}
    }
    TabBar { id: t
        onCurrentIndexChanged: root.seen.push("t idx " + currentIndex)
        TabButton { text: "a"; onCheckedChanged: root.seen.push("ta " + checked) }
        TabButton { text: "b" }
    }
    Component.onCompleted: {
        console.log(seen.join("; "));
        console.log(b.currentIndex, c.currentIndex, g.currentIndex, g.shown, t.currentIndex);
        root.minus = -1; c.currentIndex = -1;
        console.log(c.currentIndex, c.count);
        console.log(seen.join("; "));
        Qt.quit();
    }
}
