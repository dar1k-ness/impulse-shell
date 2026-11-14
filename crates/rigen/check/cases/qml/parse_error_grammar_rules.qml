// Errors the rules of Qt's grammar report themselves (qqmljs.g): a member that is only a name,
// a function with no name, a regular expression with no end.
// Each entry is a document of `parse_errors/`: its name, the status of its Component
// (3: Error) and errorString() without the directory of its url.
// qt: S05 3 "S05.qml:3:5: Incomplete binding, expected token `:` or `{`\n" S07 3 "S07.qml:3:5: Expected a qualified name id\n" S09 3 "S09.qml:3:5: Incomplete binding, expected token `:` or `{`\n" S31 3 "S31.qml:3:8: Unterminated regular expression literal\n" S37 3 "S37.qml:3:5: Incomplete binding, expected token `:` or `{`\n" S39 3 "S39.qml:3:5: Incomplete binding, expected token `:` or `{`\n"
import QtQuick
Item {
    property string result: ["S05", "S07", "S09", "S31", "S37", "S39"].map(function(n) {
        var c = Qt.createComponent("parse_errors/" + n + ".qml")
        return n + " " + c.status + " " + JSON.stringify(c.errorString().split(String(c.url).slice(0, -(n + ".qml").length)).join(""))
    }).join(" ")
}
