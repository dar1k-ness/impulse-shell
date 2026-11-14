import QtQuick
Item {
    property alias inner: img
    Image { id: img; source: "m.png" }
    function f(u, c) { return Qt.resolvedUrl(u, c) }
}
