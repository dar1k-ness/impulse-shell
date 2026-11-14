import QtQuick
import QtQuick.Controls
Item {
    TextArea { id: a; text: "hello" }
    TextEdit { id: e; text: "hello world" }
    Component.onCompleted: {
        a.cursorPosition = 5; a.text = ""
        console.log("TextArea after text='':", a.cursorPosition, a.length)
        a.insert(a.cursorPosition, "x"); console.log("insert at cursor:", JSON.stringify(a.text), a.cursorPosition)
        e.select(2, 8); e.text = "abc"
        console.log("TextEdit:", e.cursorPosition, e.selectionStart, e.selectionEnd, JSON.stringify(e.selectedText))
        e.cursorPosition = 3; e.clear(); console.log("clear():", e.cursorPosition)
    }
}
