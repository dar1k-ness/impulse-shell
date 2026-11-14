// Properties whose setters write each other: the literals are written in the order of the
// binding table (the last in the file first, all before the children), so the first in the
// file stays and a child hears nothing of what was written before it existed.
// qt: f.visible f.enabled h.visible h.enabled true 2 false 1 12- 12- false false false false
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string result: ""
    function add(x) { result += (result ? " " : "") + x }
    CheckBox { id: a; checked: true; checkState: Qt.PartiallyChecked }
    CheckBox { id: b; checkState: Qt.PartiallyChecked; checked: true }
    TextInput { id: c; text: "12ab"; inputMask: "99-99" }
    TextInput { id: d; inputMask: "99-99"; text: "12ab" }
    Item {
        id: e
        Item {
            id: f
            onVisibleChanged: root.add("f.visible")
            onEnabledChanged: root.add("f.enabled")
        }
        visible: false
        enabled: false
        onVisibleChanged: root.add("e.visible")
    }
    Item {
        id: g
        visible: e.width == 0 ? false : true
        enabled: e.width == 0 ? false : true
        Item {
            id: h
            onVisibleChanged: root.add("h.visible")
            onEnabledChanged: root.add("h.enabled")
        }
    }
    Component.onCompleted: add([a.checked, a.checkState, b.checked, b.checkState, c.text, d.text,
        f.visible, f.enabled, h.visible, h.enabled].join(" "))
}
