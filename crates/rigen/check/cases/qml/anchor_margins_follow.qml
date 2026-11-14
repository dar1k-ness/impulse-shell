// A side margin of QQuickAnchors nothing set reads `anchors.margins`; a set one reads its own
// value until it is reset; the two offsets have no wider property.
// qt: 11 11 11 11 11 0 0 0 | 5 2 5 5 5 0 0 0 | 0 0 0 0 0 0 0 0 | 7 7 7 7 7 0 0 0 | 0 0 3 0 0 0 0 0 | 9 9 7 9 9 0 0 0 | 4 4 4 4 4 0 0 0
import QtQuick
Item {
    id: root
    property real m: 5
    Item { id: a; anchors.margins: 11 }
    Item { id: b; anchors.margins: root.m; anchors.leftMargin: 2 }
    Item { id: c }
    function all(i) {
        return [i.anchors.margins, i.anchors.leftMargin, i.anchors.rightMargin, i.anchors.topMargin, i.anchors.bottomMargin,
                i.anchors.horizontalCenterOffset, i.anchors.verticalCenterOffset, i.anchors.baselineOffset].join(" ")
    }
    property string result
    Component.onCompleted: {
        var l = [all(a), all(b), all(c)]
        b.anchors.leftMargin = undefined; m = 7; c.anchors.rightMargin = 3
        l.push(all(b)); l.push(all(c))
        b.anchors.rightMargin = 7; m = 9; c.anchors.margins = 4; c.anchors.rightMargin = undefined
        l.push(all(b)); l.push(all(c))
        result = l.join(" | ")
    }
}
