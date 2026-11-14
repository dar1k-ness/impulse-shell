import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int b: 2
    Item { id: other; property string t: "t0" }
    Synchronizer { id: sy; sourceObject: root; sourceProperty: "b"; targetObject: other; targetProperty: "t" }
    onBChanged: console.log("bChanged", b)
    Component.onCompleted: {
        console.log("start b", b, "t", other.t, "valueBounced", typeof sy.valueBounced);
        b = 9; console.log("b=9 ->", b, other.t);
        other.t = "12"; console.log("t=12 ->", b, other.t);
        t.start()
    }
    Timer { id: t; interval: 30; onTriggered: { console.log("later", root.b, other.t); console.log("END"); Qt.quit() } }
}
