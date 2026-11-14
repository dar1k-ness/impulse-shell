// `p = Qt.binding(f)` evaluates the new binding inside the assignment: what depends on it is
// up to date in the next statement and its change handler has run, once.
// qt: d2 | d10 1,10,11,10 | d100 1,100,101,100 | d101 d701 7,700,701,701 | d6 7,700,6,6 | d8 9,900,8,8
import QtQuick
Item {
    id: root
    property string result: ""
    property int a: 1
    property int b: a + 1
    property int c: b + 1
    property int d: a > 5 ? c : b
    onDChanged: result += "d" + d + " "
    Component.onCompleted: {
        result += "| "
        b = 10
        result += [a, b, c, d] + " | "
        b = Qt.binding(function() { return a * 100 })
        result += [a, b, c, d] + " | "
        a = 7
        result += [a, b, c, d] + " | "
        c = Qt.binding(() => root.a - 1)
        result += [a, b, c, d] + " | "
        a = 9
        result += [a, b, c, d]
    }
}
