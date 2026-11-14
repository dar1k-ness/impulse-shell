import QtQuick
Item {
    id: root
    property int _x: 0
    property int __y: 0
    signal _sig()
    on_XChanged: console.log("on_XChanged", _x)
    on__YChanged: console.log("on__YChanged", __y)
    on_Sig: console.log("on_Sig")
    Component.onCompleted: { _x = 5; __y = 6; _sig(); console.log("END"); Qt.quit() }
}
