import QtQuick
Item {
    id: r
    property alias m: sub.anchors.margins
    property alias bw: rect.border.width
    property alias sw: img.sourceSize.width
    Item { id: sub }
    Rectangle { id: rect; border.width: 2 }
    Image { id: img }
    Component.onCompleted: { console.log(m, bw, sw); m = 3; bw = 4; sw = 5; console.log(sub.anchors.margins, rect.border.width, img.sourceSize.width) }
}
