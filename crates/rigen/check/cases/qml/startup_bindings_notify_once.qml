// Loading runs each binding once: an object- or array-valued binding notifies once, like a
// string one (no second pass over every binding after load).
// qt: oas
import QtQuick
Item {
    property string result: ""
    property int k: 1
    property var o: ({ v: k })
    onOChanged: result += "o"
    property var arr: [k, 2]
    onArrChanged: result += "a"
    property string s: "x" + k
    onSChanged: result += "s"
}
