import QtQuick
import QtQuick.Layouts

Item {
    id: root
    width: 200; height: 100
    property int steps: 6
    property int fw: 0
    property int fh: 0
    property int lrw: 0
    Item { id: f; implicitWidth: 7; implicitHeight: 8; anchors.fill: parent
        onWidthChanged: root.fw++; onHeightChanged: root.fh++ }
    Item { id: fm; anchors.fill: parent; anchors.margins: 5; anchors.leftMargin: 2 }
    Item { id: c; implicitWidth: 20; implicitHeight: 10; anchors.centerIn: parent }
    Text { id: t; text: "x"; anchors.baseline: parent.verticalCenter }
    Item { id: lr; implicitWidth: 9; height: 4; anchors.left: parent.left; anchors.right: parent.right
        onWidthChanged: root.lrw++ }
    Item { id: edges; anchors.left: parent.left; anchors.right: parent.right; anchors.top: parent.top; anchors.bottom: parent.bottom }
    Item { id: mid; implicitWidth: 3; implicitHeight: 4; anchors.left: parent.left; anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top; anchors.verticalCenter: parent.verticalCenter }
    Item { id: bound; anchors.fill: root.steps > 0 ? parent : undefined }
    RowLayout { id: row; width: 120; height: 40
        Item { id: la; Layout.fillWidth: true; Layout.fillHeight: true }
        Item { id: lb; implicitWidth: 30; implicitHeight: 5 }
    }
    function g(i) { return [i.x, i.y, i.width, i.height].join(" ") }
    function step(n) {
        var l = []
        if (n == 0) {
            l.push("f " + g(f) + " changes " + fw + " " + fh)
            l.push("fm " + g(fm) + " c " + g(c) + " t " + t.x + " " + (t.y + t.baselineOffset))
            l.push("lr " + g(lr) + " changes " + lrw + " edges " + g(edges) + " mid " + g(mid) + " bound " + g(bound))
            l.push("la " + g(la) + " lb " + g(lb))
            f.anchors.fill = undefined
            fm.anchors.leftMargin = undefined
            c.anchors.centerIn = undefined
            t.anchors.baseline = undefined
            lr.anchors.right = undefined
            edges.anchors.left = undefined; edges.anchors.bottom = undefined
            mid.anchors.horizontalCenter = undefined; mid.anchors.top = undefined
            steps = 0
        } else if (n == 1) {
            l.push("f " + g(f) + " changes " + fw + " " + fh)
            l.push("fm " + g(fm) + " c " + g(c) + " t " + t.x + " " + (t.y + t.baselineOffset))
            l.push("lr " + g(lr) + " changes " + lrw + " edges " + g(edges) + " mid " + g(mid) + " bound " + g(bound))
            f.implicitWidth = 50; f.implicitHeight = 60
            c.implicitWidth = 40
            lr.implicitWidth = 33
            mid.implicitWidth = 6; mid.implicitHeight = 7
        } else if (n == 2) {
            l.push("f " + g(f) + " changes " + fw + " " + fh)
            l.push("c " + g(c) + " lr " + g(lr) + " changes " + lrw + " mid " + g(mid))
            root.width = 300; root.height = 150
        } else if (n == 3) {
            l.push("f " + g(f) + " fm " + g(fm) + " lr " + g(lr) + " edges " + g(edges) + " mid " + g(mid) + " bound " + g(bound))
            f.width = undefined; lr.width = undefined; edges.height = undefined; mid.width = undefined; bound.height = undefined
        } else if (n == 4) {
            l.push("f " + g(f) + " changes " + fw + " " + fh)
            l.push("lr " + g(lr) + " changes " + lrw + " edges " + g(edges) + " mid " + g(mid) + " bound " + g(bound))
            f.anchors.fill = root; lr.anchors.right = root.right
            row.width = 150
        } else if (n == 5) {
            l.push("f " + g(f) + " changes " + fw + " " + fh + " lr " + g(lr) + " changes " + lrw)
            l.push("la " + g(la) + " lb " + g(lb))
        }
        return l.join("\n")
    }
}
