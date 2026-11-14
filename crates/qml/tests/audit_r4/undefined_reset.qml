import QtQuick
import QtQuick.Layouts
Item {
    id: root
    property bool on: true
    width: 200; height: 100
    Rectangle { id: r; width: root.on ? 50 : undefined; height: root.on ? 20 : undefined; implicitWidth: 33; implicitHeight: 44
        anchors.left: root.on ? undefined : parent.left
        onWidthChanged: console.log("r.widthChanged", width) }
    Text { id: t; text: "hello"; font.pixelSize: root.on ? 30 : undefined; elide: root.on ? Text.ElideRight : undefined; horizontalAlignment: root.on ? Text.AlignRight : undefined }
    Component.onCompleted: {
        console.log("start", r.width, r.height, t.font.pixelSize, t.elide, t.horizontalAlignment);
        on = false;
        console.log("off", r.width, r.height, t.font.pixelSize, t.elide, t.horizontalAlignment);
        on = true;
        console.log("on again", r.width, r.height, t.font.pixelSize, t.elide, t.horizontalAlignment);
        console.log("END"); Qt.quit();
    }
}
