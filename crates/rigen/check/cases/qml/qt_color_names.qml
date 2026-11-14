// Qt.color(name) (QtObject::color): the argument as a string, and an error for one that names
// no colour; `undefined` and `null` name none.
// qt: !Error: "undefined" is not a valid color name | !Error: "null" is not a valid color name | !Error: "" is not a valid color name | !Error: "nope" is not a valid color name | !Error: "5" is not a valid color name | object:#ff0000 | !Error: "#12" is not a valid color name | object:#112233 | object:#102030 | !Error: "[object Object]" is not a valid color name | !Error: Insufficient arguments | object:#00000000 | !Error: "true" is not a valid color name | object:#80102030 | object:#ff0000 | object:#ff0000 | !Error: "1" is not a valid color name | !Error: "1" is not a valid color name
import QtQuick
Item {
    id: root
    property color c: "#102030"
    function t(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    property string result: [
        () => Qt.color(undefined), () => Qt.color(null), () => Qt.color(""), () => Qt.color("nope"),
        () => Qt.color(5), () => Qt.color("red"), () => Qt.color("#12"), () => Qt.color("#123"),
        () => Qt.color(root.c), () => Qt.color({}), () => Qt.color(), () => Qt.color("transparent"),
        () => Qt.color(true), () => Qt.color("#80102030"), () => Qt.color(" red"), () => Qt.color("RED"),
        () => Qt.color([1]), () => Qt.color("1")
    ].map(t).join(" | ")
}
