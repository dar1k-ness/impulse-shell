// A value type has no signals: a handler on a property of `sourceSize` of an Image is a load error,
// as in Qt 6.12 (which names the file, line and column before the message).
// qt: load error Cannot assign to non-existent property "onWidthChanged"
import QtQuick
Item {
    property string result: "loaded"
    Image { sourceSize.onWidthChanged: console.log("x") }
}
