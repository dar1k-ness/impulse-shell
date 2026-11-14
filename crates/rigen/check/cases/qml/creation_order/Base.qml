// A type from QML with handlers of its own: they are connected before the document that
// uses the type writes anything.
import QtQuick
Item {
    property string tag: ""
    property int a: 0
    property int b: 0
    property int e: a * 10
    onTagChanged: Log.add(tag)
    onAChanged: Log.add("base.a=" + a)
    onBChanged: Log.add("base.b=" + b)
    onEChanged: Log.add("base.e=" + e)
    Component.onCompleted: Log.add("base.done")
}
