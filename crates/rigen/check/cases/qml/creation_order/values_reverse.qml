// QmlIR::Object::appendBinding prepends a binding that is not to the default property: the
// literals of one object are written last in the file first. The handlers of the type's own
// document hear them; the ones written next to them are connected after the values
// (QmlUnitGenerator::generate) and hear nothing.
// qt: base.b=2 base.a=1 base.e=10 base.done
import QtQuick
Base {
    property string result: Log.s
    a: 1
    b: 2
    onAChanged: Log.add("own.a")
    onBChanged: Log.add("own.b")
}
