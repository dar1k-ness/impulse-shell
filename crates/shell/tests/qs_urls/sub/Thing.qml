import QtQuick
Item {
    property string own: Qt.resolvedUrl("pic.png")
    property alias inner: img
    Image { id: img; source: "pic.png" }
    function f(u) { return Qt.resolvedUrl(u) }
}
