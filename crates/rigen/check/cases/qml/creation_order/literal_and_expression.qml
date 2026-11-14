// A literal is written while the object is populated, before its handlers are connected;
// an expression waits for QQmlObjectCreator::finalize, which enables the last created
// binding first.
// qt: e1 e2 e3
import QtQuick
Item {
    property string result: Log.s
    property int one: 1
    property int e1: one + 1
    property int lit: 7
    property int e2: one + 2
    property int e3: one + 3
    onE1Changed: Log.add("e1")
    onLitChanged: Log.add("lit")
    onE2Changed: Log.add("e2")
    onE3Changed: Log.add("e3")
}
