import QtQuick
import QtQuick.Controls

// What `step(k)` gives in Qt 6.12 is in qt.txt.
Item {
    id: root
    width: 200; height: 200
    property int steps: 6
    Item { id: other; x: 50; y: 60; width: 80; height: 70 }
    // The height is the implicit one, not a set one: the text is not moved down in it.
    TextField { id: implicitField; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset" }
    TextField { id: setField; padding: 5; width: 31; height: 32; text: "preset" }
    TextInput { id: setInput; width: 40; height: 40; verticalAlignment: TextInput.AlignBottom; text: "preset" }
    Text { id: setText; height: 40; verticalAlignment: Text.AlignVCenter; text: "preset" }
    Item { id: filled; implicitWidth: 60; implicitHeight: 40
        TextInput { id: anchored; anchors.fill: parent; verticalAlignment: TextInput.AlignVCenter; text: "preset" } }
    Popup { id: popup; implicitWidth: 31; implicitHeight: 32 }
    Menu { id: menu; implicitWidth: 31; implicitHeight: 32 }
    function g(o) { return [o.x, o.y, o.width, o.height, o.baselineOffset].join(",") }
    function p(o) { return [o.x, o.y, o.width, o.height, o.visible, o.anchors.centerIn === other, o.anchors.centerIn === root].join(",") }
    // The writes of step k are read by step k + 1: an anchor places its item in the next frame.
    function step(k) {
        const was = [g(implicitField), g(setField), g(setInput), g(setText), g(anchored), p(popup), p(menu)].join("\n")
        if (k == 0) {
            for (const o of [implicitField, setField, setInput, setText])
                o.anchors.baseline = other.top
            popup.anchors.centerIn = other
            menu.anchors.centerIn = root
        }
        if (k == 1) { popup.open(); menu.open() }
        if (k == 2) { popup.close(); menu.close() }
        if (k == 3) { popup.anchors.centerIn = undefined; menu.anchors.centerIn = undefined; implicitField.implicitHeight = 50 }
        return was
    }
}
