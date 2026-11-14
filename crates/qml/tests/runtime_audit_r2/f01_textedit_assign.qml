import QtQuick
import QtQuick.Controls
Item {
    function st(t) { return JSON.stringify(t.text) + " c" + t.cursorPosition + " s" + t.selectionStart + "-" + t.selectionEnd + " " + JSON.stringify(t.selectedText) + " len" + t.length }
    TextEdit { id: e; text: "hello world"
        onTextChanged: console.log("  e.text", st(e))
        onCursorPositionChanged: console.log("  e.cursor", st(e))
        onSelectionStartChanged: console.log("  e.selStart", st(e))
        onSelectionEndChanged: console.log("  e.selEnd", st(e))
        onSelectedTextChanged: console.log("  e.selText", st(e))
        onLengthChanged: console.log("  e.length", st(e))
    }
    TextArea { id: a; text: "hello world"
        onTextChanged: console.log("  a.text", st(a))
        onCursorPositionChanged: console.log("  a.cursor", st(a))
        onSelectionStartChanged: console.log("  a.selStart", st(a))
        onSelectionEndChanged: console.log("  a.selEnd", st(a))
        onSelectedTextChanged: console.log("  a.selText", st(a))
    }
    Component.onCompleted: {
        console.log("select 2,8"); e.select(2, 8); console.log("set abc"); e.text = "abc"; console.log("=", st(e))
        console.log("select 3,0"); e.select(3, 0); console.log("set longer"); e.text = "abcdefgh"; console.log("=", st(e))
        console.log("select 0,3"); e.select(0, 3); console.log("set same"); e.text = "abcdefgh"; console.log("=", st(e))
        console.log("select 1,3"); e.select(1, 3); console.log("set empty"); e.text = ""; console.log("=", st(e))
        e.text = "one two"
        console.log("selectAll"); e.selectAll(); console.log("clear"); e.clear(); console.log("=", st(e))
        e.text = "one two"
        console.log("select 2,4"); e.select(2, 4); console.log("set same length"); e.text = "ONE TWO"; console.log("=", st(e))
        console.log("cursor 3"); e.cursorPosition = 3; console.log("set, no selection"); e.text = "q"; console.log("=", st(e))
        console.log("a select 2,8"); a.select(2, 8); console.log("a set abc"); a.text = "abc"; console.log("=", st(a))
        console.log("a selectAll"); a.selectAll(); console.log("a set empty"); a.text = ""; console.log("=", st(a))
    }
}
