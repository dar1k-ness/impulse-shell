// cut() and copy() of TextEdit and TextInput: the signals each sends and what its handlers see,
// with and without `text` read before the call. The clipboard is in clip.qml. Both run in
// runtime.rs (cut_and_copy_*), where the clipboard is a list of the test.
import QtQuick
Item {
    id: root
    property string log: ""
    property bool on: false
    function ev(n, t) { if (on) log += " " + n + "(" + t.length + "|" + t.text + "|" + t.cursorPosition + "," + t.selectionStart + "," + t.selectionEnd + "|" + t.selectedText.length + ")" }
    TextEdit {
        id: te
        text: "hello world"
        onTextChanged: root.ev("text", te)
        onCursorPositionChanged: root.ev("cp", te)
        onSelectionStartChanged: root.ev("ss", te)
        onSelectionEndChanged: root.ev("se", te)
        onSelectedTextChanged: root.ev("st", te)
    }
    TextInput {
        id: ti
        text: "hello world"
        onTextChanged: root.ev("text", ti)
        onTextEdited: root.ev("edited", ti)
        onCursorPositionChanged: root.ev("cp", ti)
        onSelectionStartChanged: root.ev("ss", ti)
        onSelectionEndChanged: root.ev("se", ti)
        onSelectedTextChanged: root.ev("st", ti)
    }
    TextEdit { id: sinkE }
    TextInput { id: sinkI }
    function st(t) { return "(" + t.length + "|" + t.text + "|" + t.cursorPosition + "," + t.selectionStart + "," + t.selectionEnd + "|" + t.selectedText.length + ")" }
    function step(name, t, pre, read, f) {
        t.readOnly = false
        if (t === ti) t.echoMode = TextInput.Normal
        t.text = "hello world"; pre(t)
        var r = ""
        if (read === 1) r = t.text
        log += name + " read=" + read + " " + st(t) + " ->"
        on = true
        try { f(t) } catch (e) { log += " THROW " + e }
        on = false
        log += " after" + st(t) + "\n"
    }
    Component.onCompleted: {
        var pres = [
            ["sel(2,4)", t => t.select(2, 4)],
            ["sel(4,2)", t => t.select(4, 2)],
            ["nosel@5", t => { t.cursorPosition = 5 }],
            ["all", t => t.selectAll()],
            ["ro sel(2,4)", t => { t.readOnly = true; t.select(2, 4) }],
            ["nosel@11", t => { t.cursorPosition = 11 }],
            ["sel(0,5)@0", t => { t.select(5, 0) }],
            ["sel(6,11)", t => { t.select(6, 11) }],
            ["password sel(2,4)", t => { if (t === ti) t.echoMode = TextInput.Password; t.select(2, 4) }]
        ]
        var ops = [
            ["cut", t => t.cut()],
            ["copy", t => t.copy()]
        ]
        var ts = [["te", te], ["ti", ti]]
        for (var a = 0; a < 2; a++)
            for (var p = 0; p < pres.length; p++)
                for (var i = 0; i < ops.length; i++)
                    for (var r = 0; r < 2; r++)
                        step(ts[a][0] + " " + pres[p][0] + " " + ops[i][0], ts[a][1], pres[p][1], r, ops[i][1])
        console.log("LOG<" + log + ">LOG"); Qt.quit()
    }
}
