// The same recovery of Qt's parser ("Expected token") in object members, imports and the
// document's root.
// Each entry is a document of `parse_errors/`: its name, the status of its Component
// (3: Error) and errorString() without the directory of its url.
// qt: S03 3 "S03.qml:3:11: Expected token `:'\n" S04 3 "S04.qml:4:1: Expected token `}'\n" S06 3 "S06.qml:4:1: Expected token `identifier'\n" S08 3 "S08.qml:4:1: Expected token `)'\n" S15 3 "S15.qml:2:1: Expected token `numeric literal'\n" S16 3 "S16.qml:1:1: Expected token `numeric literal'\n" S21 3 "S21.qml:2:6: Expected token `;'\n" S22 3 "S22.qml:2:1: Expected token `version number'\n" S26 3 "S26.qml:3:17: Expected token `identifier'\n" S35 3 "S35.qml:4:1: Expected token `identifier'\n" S36 3 "S36.qml:3:17: Expected token `identifier'\n"
import QtQuick
Item {
    property string result: ["S03", "S04", "S06", "S08", "S15", "S16", "S21", "S22", "S26", "S35", "S36"].map(function(n) {
        var c = Qt.createComponent("parse_errors/" + n + ".qml")
        return n + " " + c.status + " " + JSON.stringify(c.errorString().split(String(c.url).slice(0, -(n + ".qml").length)).join(""))
    }).join(" ")
}
