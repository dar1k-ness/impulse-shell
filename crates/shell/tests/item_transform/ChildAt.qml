// Item.childAt over children whose size or place comes from layout: a Loader sized by its
// item, the cells of a Row, a Text sized by its words. tests/item_transform.rs asks after
// the window is laid out.
// qt: loader loader null null | row row null | c0 null c1 null | tx null | sized null
import QtQuick
Item {
    id: root
    width: 300; height: 300
    property string result: ""
    property int go: 0
    function n(o) { return o === null ? "null" : o.objectName }
    function probe() {
        var at = (o, pts) => pts.map(p => n(o.childAt(p[0], p[1]))).join(" ")
        return [
            at(root, [[65, 5], [89, 29], [90, 5], [65, 30]]),
            at(root, [[5, 255], [20, 255], [26, 255]]),
            at(row, [[2, 2], [12, 2], [16, 2], [25, 2]]),
            at(root, [[152, 152], [149, 152]]),
            at(root, [[225, 205], [231, 205]])
        ].join(" | ")
    }
    onGoChanged: result = probe()
    Component.onCompleted: go = 1
    Loader { objectName: "loader"; x: 60; sourceComponent: Item { objectName: "loaded"; implicitWidth: 30; implicitHeight: 30 } }
    Row {
        id: row; objectName: "row"; x: 0; y: 250; spacing: 5
        Item { objectName: "c0"; width: 10; height: 10 }
        Item { objectName: "c1"; width: 10; height: 10 }
    }
    Text { objectName: "tx"; x: 150; y: 150; text: "hello" }
    Item { objectName: "sized"; x: 200; y: 200; implicitWidth: 30; implicitHeight: 30 }
}
