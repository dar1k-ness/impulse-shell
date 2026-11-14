// A number written to a string role of a ListModel (Qt 6.12): QVariant(double).toString().
// qt: 12345670000 12345670000;12345600000 12345600000;12345000000 1.2345e+10;12340000000 1.234e+10;5 5;4294967296 4294967296;3000000000 3e+09;2147483648 2147483648;2147483647 2147483647;-2147483648 -2147483648;-2147483649 -2147483649;1e+21 1e+21;1.25 1.25;1e+300 1e+300;5e-324 5e-324;0.5 0.5;1234567.5 1234567.5
import QtQuick
Item {
    id: root
    property var seen: []
    property string result: seen.join(";")
    ListModel { id: lm; ListElement { str: "a" } }
    Component.onCompleted: {
        var xs = [12345670000, 12345600000, 12345000000, 12340000000, 2.5*2, 4294967296, 3000000000, 2147483648, 2147483647, -2147483648, -2147483649, 1e21+1, 1.25, 1e300, 5e-324, 0.5, 1234567.5]
        for (var i = 0; i < xs.length; i++) { lm.setProperty(0, "str", xs[i]); seen.push(xs[i] + " " + lm.get(0).str) }
        seenChanged()
    }
}
