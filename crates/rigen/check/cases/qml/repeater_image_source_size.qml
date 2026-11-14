// Every delegate of a Repeater has the `sourceSize` its Image asks for with a binding, the
// fifth as the first.
// qt: 24x24 24x24 24x24 24x24 24x24 | 7x9 7x9 7x9 7x9 7x9
import QtQuick
Item {
    id: root
    property string result: ""
    Repeater {
        id: whole
        model: 5
        Item { property size s: im.sourceSize; Image { id: im; sourceSize: Qt.size(24, 24) } }
    }
    Repeater {
        id: text
        model: 5
        Item { property size s: im2.sourceSize; Image { id: im2; sourceSize: 3 + 4 + "x9" } }
    }
    function f(r) {
        let a = []
        for (let i = 0; i < r.count; i++) a.push(r.itemAt(i).s.width + "x" + r.itemAt(i).s.height)
        return a.join(" ")
    }
    Component.onCompleted: result = f(whole) + " | " + f(text)
}
