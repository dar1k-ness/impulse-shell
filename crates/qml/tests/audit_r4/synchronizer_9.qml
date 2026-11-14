import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int a: 1
    property int b: 2
    property int e: 0
    onAChanged: console.log("aChanged", a)
    onBChanged: console.log("bChanged", b)
    onEChanged: console.log("eChanged", e)
    Item { id: other; width: 3; property int c: 0
        onWidthChanged: console.log("widthChanged", width)
        onCChanged: console.log("cChanged", c)
        Synchronizer on width { property alias source: root.a; property alias second: root.b }
        Synchronizer { sourceObject: root; sourceProperty: "e"; targetObject: other; targetProperty: "height" }
        onHeightChanged: console.log("heightChanged", height)
    }
    Component.onCompleted: {
        console.log("start", a, b, other.width);
        b = 5; console.log("b=5", a, b, other.width);
        e = 6; console.log("e=6", e, other.height);
        other.height = 8; console.log("height=8", e, other.height);
        console.log("END"); Qt.quit();
    }
}
