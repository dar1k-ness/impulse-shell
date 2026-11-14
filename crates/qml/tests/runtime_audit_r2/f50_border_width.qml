import QtQuick
Item {
    id: r
    property alias bw: rect.border.width
    Rectangle { id: rect }
    Rectangle { id: set; border.width: 1 }
    Rectangle { id: zero; border.width: 0 }
    Rectangle { id: col; border.color: "red" }
    Item { id: a; Item {} Item { visible: false } }
    Component.onCompleted: {
        console.log("default", rect.border.width, set.border.width, zero.border.width, col.border.width, rect.border.color, rect.border.pixelAligned, bw)
        console.log("visibleChildren", a.visibleChildren.length, "childAt", typeof r.childAt)
        rect.border.width = 3
        console.log(rect.border.width, bw)
        rect.border.width = 0
        rect.border.width = -2
        console.log("end", rect.border.width, bw)
    }
}
