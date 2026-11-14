// A value type has no signals: a handler on a property of `font { }` of a Text is a load error,
// as in Qt 6.12 (which names the file, line and column before the message).
// qt: load error Cannot assign to non-existent property "onBoldChanged"
import QtQuick
Item {
    property string result: "loaded"
    Text { font { pixelSize: 3; onBoldChanged: console.log("x") } }
}
