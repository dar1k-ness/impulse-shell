// Qt's parser recovers from a syntax error by inserting a token its tables accept (qqmljs.g,
// Parser::parse): the error is "Expected token" with the inserted token, at the token it stopped
// on. Bindings and JavaScript.
// Each entry is a document of `parse_errors/`: its name, the status of its Component
// (3: Error) and errorString() without the directory of its url.
// qt: S01 3 "S01.qml:4:1: Expected token `;'\n" S02 3 "S02.qml:4:1: Expected token `numeric literal'\n" S10 3 "S10.qml:3:8: Expected token `;'\n" S11 3 "S11.qml:4:1: Expected token `,'\n" S12 3 "S12.qml:4:1: Expected token `,'\n" S17 3 "S17.qml:3:10: Expected token `;'\n" S18 3 "S18.qml:4:17: Expected token `numeric literal'\n" S19 3 "S19.qml:4:19: Expected token `numeric literal'\n" S20 3 "S20.qml:5:5: Expected token `;'\n" S27 3 "S27.qml:4:14: Expected token `,'\n" S29 3 "S29.qml:5:1: Expected token `}'\n" S30 3 "S30.qml:3:8: Expected token `;'\n" S33 3 "S33.qml:3:8: Expected token `;'\n" S40 3 "S40.qml:4:1: Expected token `)'\n"
import QtQuick
Item {
    property string result: ["S01", "S02", "S10", "S11", "S12", "S17", "S18", "S19", "S20", "S27", "S29", "S30", "S33", "S40"].map(function(n) {
        var c = Qt.createComponent("parse_errors/" + n + ".qml")
        return n + " " + c.status + " " + JSON.stringify(c.errorString().split(String(c.url).slice(0, -(n + ".qml").length)).join(""))
    }).join(" ")
}
