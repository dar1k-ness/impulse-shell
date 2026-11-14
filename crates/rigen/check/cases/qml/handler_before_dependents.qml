// A change signal calls what is connected to it in connection order: the handler the object
// declares was connected when the object was created, the bindings that read the property
// when they first ran. The handler still reads those bindings' old values.
// qt: [ x=1,yy=0 yy=2 it=true,zz=-1 it=false,zz=5 item=true,w=-1 item=false,w=5 ] 2 -1 -1
import QtQuick
Item {
    id: root
    property string result: ""
    function add(x) { result += (result ? " " : "") + x }
    QtObject {
        id: o
        property int x: 0
        property QtObject it: null
        onXChanged: root.add("x=" + x + ",yy=" + root.yy)
        onItChanged: root.add("it=" + (it !== null) + ",zz=" + root.zz)
    }
    property int yy: o.x * 2
    property int zz: o.it ? 5 : -1
    onYyChanged: add("yy=" + yy)
    Loader {
        id: ld
        active: false
        sourceComponent: Item { property int v: 5 }
        onItemChanged: root.add("item=" + (item !== null) + ",w=" + root.w)
    }
    property int w: ld.item ? ld.item.v : -1
    Component.onCompleted: {
        add("[")
        o.x = 1; o.it = root; o.it = null; ld.active = true; ld.active = false
        add("] " + yy + " " + zz + " " + w)
    }
}
