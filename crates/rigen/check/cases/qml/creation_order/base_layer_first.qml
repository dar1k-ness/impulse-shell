// QQmlObjectCreator::createInstance builds the whole document of the type first: its
// handlers hear the literals of the document that uses it, its bindings are enabled after
// the ones written here.
// qt: base.a=2 own.f base.e=20 own.e base.done own.done
import QtQuick
Base {
    property string result: Log.s
    a: 2
    property int f: a + 1
    onAChanged: Log.add("own.a")
    onEChanged: Log.add("own.e")
    onFChanged: Log.add("own.f")
    Component.onCompleted: Log.add("own.done")
}
