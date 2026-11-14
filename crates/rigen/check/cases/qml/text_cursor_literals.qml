// `text` and `cursorPosition` literals of one object. Qt sets the later line of the file
// first. A TextInput moves the cursor to the end of a new text, so a `cursorPosition` written
// above `text` is the one that stays. A TextEdit only stores the text until
// componentComplete: its document is still empty, and a position past its end is dropped.
// qt: 2 1 2 1 0 0 0 0 | 1 2
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string result: ""
    TextField { id: f1; text: "<>"; cursorPosition: 1 }
    TextField { id: f2; cursorPosition: 1; text: "<>" }
    TextInput { id: i1; text: "<>"; cursorPosition: 1 }
    TextInput { id: i2; cursorPosition: 1; text: "<>" }
    TextArea { id: a1; text: "cd"; cursorPosition: 1 }
    TextArea { id: a2; cursorPosition: 1; text: "cd" }
    TextEdit { id: e1; text: "cd"; cursorPosition: 1 }
    TextEdit { id: e2; cursorPosition: 1; text: "cd" }
    Component.onCompleted: {
        var made = [f1, f2, i1, i2, a1, a2, e1, e2].map(o => o.cursorPosition).join(" ")
        a2.cursorPosition = 1
        e2.cursorPosition = 2
        result = made + " | " + a2.cursorPosition + " " + e2.cursorPosition
    }
}
