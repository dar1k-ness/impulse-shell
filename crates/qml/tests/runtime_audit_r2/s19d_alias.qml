import QtQuick
Item {
    id: r
    property alias inner: sub
    Item { id: sub }
    Text { id: t }
    Component.onCompleted: {
        try { inner = t; console.log("no error, inner === t:", inner === t) } catch (e) { console.log("error:", e.message) }
    }
}
