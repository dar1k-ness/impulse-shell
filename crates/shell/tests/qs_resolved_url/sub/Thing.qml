import QtQuick
Item {
    property alias inner: img
    Image { id: img; source: "pic.png" }
    function f(u, c) { return Qt.resolvedUrl(u, c) }
}
