import QtQuick
Item {
    id: r
    property alias fs: t.font.pixelSize
    Text { id: t }
    Component.onCompleted: { console.log(fs); fs = 20; console.log(t.font.pixelSize) }
}
