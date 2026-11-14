// When no inserted token helps, Qt's parser drops the token it stopped on if the next one fits
// ("Unexpected token"), and reports "Syntax error" otherwise.
// Each entry is a document of `parse_errors/`: its name, the status of its Component
// (3: Error) and errorString() without the directory of its url.
// qt: S13 3 "S13.qml:4:1: Syntax error\n" S28 3 "S28.qml:3:10: Unexpected token `;'\n" S32 3 "S32.qml:4:1: Syntax error\n" S34 3 "S34.qml:3:14: Unexpected token `identifier'\n" S38 3 "S38.qml:3:23: Syntax error\n"
import QtQuick
Item {
    property string result: ["S13", "S28", "S32", "S34", "S38"].map(function(n) {
        var c = Qt.createComponent("parse_errors/" + n + ".qml")
        return n + " " + c.status + " " + JSON.stringify(c.errorString().split(String(c.url).slice(0, -(n + ".qml").length)).join(""))
    }).join(" ")
}
