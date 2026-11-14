import QtQuick
Item {
    function st(t) { return JSON.stringify(t.text) + " c" + t.cursorPosition + " s" + t.selectionStart + "-" + t.selectionEnd + " " + JSON.stringify(t.selectedText) + " len" + t.length }
    TextEdit { id: e; text: "hello world"
        onTextChanged: console.log("  e.text", st(e))
        onCursorPositionChanged: console.log("  e.cursor", st(e))
        onSelectionStartChanged: console.log("  e.selStart", st(e))
        onSelectionEndChanged: console.log("  e.selEnd", st(e))
        onSelectedTextChanged: console.log("  e.selText", st(e))
    }
    TextEdit { id: q; text: "quiet text" }
    Component.onCompleted: {
        console.log("cursor 2"); e.cursorPosition = 2; console.log("set empty, no selection"); e.text = ""; console.log("=", st(e))
        e.text = "one two"
        console.log("select 4,0"); e.select(4, 0); console.log("set empty"); e.text = ""; console.log("=", st(e))
        e.text = "one two"
        console.log("select 4,0"); e.select(4, 0); console.log("set x"); e.text = "x"; console.log("=", st(e))
        console.log("cursor 1"); e.cursorPosition = 1; console.log("=", st(e))
        console.log("set y"); e.text = "y"; console.log("=", st(e))
        e.text = "three four"
        console.log("select 0,5"); e.select(0, 5); console.log("set z"); e.text = "z"; console.log("=", st(e))
        console.log("set zz"); e.text = "zz"; console.log("=", st(e))
        console.log("selectAll"); e.selectAll(); console.log("=", st(e))
        console.log("set w"); e.text = "w"; console.log("=", st(e))
        console.log("deselect"); e.deselect(); console.log("=", st(e))
        console.log("insert"); e.insert(0, "ab"); console.log("=", st(e))
        q.select(2, 6); q.text = "other"; console.log("q", st(q))
        q.select(1, 0); q.text = "more"; console.log("q", st(q)); q.select(0, 2); console.log("q", st(q))
    }
}
