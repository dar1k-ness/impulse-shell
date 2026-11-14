import QtQuick
Item {
    id: root
    property int _z
    property int __y
    property int a
    signal _s(int n)
    signal __s()
    on_ZChanged: console.log("_z", _z)
    on__YChanged: console.log("__y", __y)
    onAChanged: console.log("a", a)
    on_S: n => console.log("_s", n)
    on__S: console.log("__s")
    Component { id: made; Item { property int _z; on_ZChanged: console.log("made _z", _z) } }
    Component.onCompleted: {
        console.log("start");
        a = 1;
        _z = 2;
        __y = 3;
        _s(4);
        __s();
        const o = made.createObject(root);
        console.log("created");
        o._z = 5;
        Qt.quit();
    }
}
