import QtQuick
Item {
    id: w
    property string own: Qt.resolvedUrl("m.png")
    property alias inner: img
    Image { id: img; source: "m.png" }
    function f(u) { return Qt.resolvedUrl(u) }
    function mk(u) { var c = Qt.createComponent(u); var x = c.status === Component.Ready ? c.createObject(w) : null; return c.status + " " + c.url + " " + (x ? x.me + " " + x.meq : "null") }
}
