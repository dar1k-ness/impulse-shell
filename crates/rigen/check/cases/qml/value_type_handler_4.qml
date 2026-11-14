// A value type has no signals: a handler on a property of a `point` property of one's own is a load error,
// as in Qt 6.12 (which names the file, line and column before the message).
// qt: load error Cannot assign to non-existent property "onXChanged"
import QtQuick
Item {
    property string result: "loaded"
    property point p
    p.onXChanged: console.log("x")
}
