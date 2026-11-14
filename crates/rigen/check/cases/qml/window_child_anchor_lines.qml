// An item declared in a Window, an ApplicationWindow or a Popup has that object's
// `contentItem` as its parent: `parent.top` is an anchor line there. An object that is not an
// item has no anchor lines.
// qt: object object object undefined undefined
import QtQuick
import QtQuick.Controls
import QtQuick.Window
Item {
    id: root
    width: 400; height: 300
    property string result: ""
    Window { id: w; Item { id: wi; anchors.top: parent.top } }
    Popup { id: p; Item { id: pi; anchors.top: parent.top } }
    ApplicationWindow { id: aw; Item { id: ai; anchors.top: parent.top } }
    IntValidator { id: iv }
    Timer { id: t }
    function f(x) { return typeof x + (x === null ? "null" : "") }
    Component.onCompleted: result = [f(wi.anchors.top), f(pi.anchors.top), f(ai.anchors.top), f(iv.left), f(t.top)].join(" ")
}
