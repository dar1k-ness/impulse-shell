// Errors of Qt's IR builder on a document its parser accepts (qqmlirbuilder.cpp): the name of an
// object's type, the value of `id`, the names of an enum's values.
// Each entry is a document of `parse_errors/`: its name, the status of its Component
// (3: Error) and errorString() without the directory of its url.
// qt: S14 3 "S14.qml:2:1: Expected type name\n" S23 3 "S23.qml:3:9: IDs must start with a letter or underscore\n" S24 3 "S24.qml:3:9: IDs cannot start with an uppercase letter\n" S25 3 "S25.qml:3:14: Enum names must begin with an upper case letter\n"
import QtQuick
Item {
    property string result: ["S14", "S23", "S24", "S25"].map(function(n) {
        var c = Qt.createComponent("parse_errors/" + n + ".qml")
        return n + " " + c.status + " " + JSON.stringify(c.errorString().split(String(c.url).slice(0, -(n + ".qml").length)).join(""))
    }).join(" ")
}
