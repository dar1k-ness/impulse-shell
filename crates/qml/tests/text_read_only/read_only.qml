// `readOnly = false` on a read-only TextInput (plain, with an input mask filled in whole and
// in part) and TextEdit: Qt puts the cursor at the end and drops the selection, sending the
// cursor's and the selection's signals before readOnlyChanged. Also the cursor set after it,
// which a mask moves to a blank. With a mask the positions are in the text shown, as long as
// the mask, but a cursor is taken only up to the length of `text`.
import QtQuick
Item {
    id: root
    property string log: ""
    property bool on: false
    function st(t) { return "(" + t.text + "|" + t.cursorPosition + "," + t.selectionStart + "," + t.selectionEnd + "|" + t.selectedText + ")" }
    function ev(n, t) { if (on) log += " " + n + st(t) }
    component Input: TextInput {
        id: i
        onTextChanged: root.ev("text", i)
        onTextEdited: root.ev("edited", i)
        onCursorPositionChanged: root.ev("cp", i)
        onSelectionStartChanged: root.ev("ss", i)
        onSelectionEndChanged: root.ev("se", i)
        onSelectedTextChanged: root.ev("st", i)
        onReadOnlyChanged: root.ev("ro", i)
    }
    Input { id: ti }
    Input { id: tm; inputMask: "AAAAA-AAAAA;_" }
    TextEdit {
        id: te
        onTextChanged: root.ev("text", te)
        onCursorPositionChanged: root.ev("cp", te)
        onSelectionStartChanged: root.ev("ss", te)
        onSelectionEndChanged: root.ev("se", te)
        onSelectedTextChanged: root.ev("st", te)
        onReadOnlyChanged: root.ev("ro", te)
    }
    readonly property var pres: [
        ["sel(2,4)", t => t.select(2, 4)],
        ["sel(4,2)", t => t.select(4, 2)],
        ["nosel@5", t => { t.cursorPosition = 5 }],
        ["nosel@0", t => { t.cursorPosition = 0 }],
        ["nosel@end", t => { t.cursorPosition = t.length }],
        ["all", t => t.selectAll()]
    ]
    readonly property var ops: [
        ["ro=false", t => { t.readOnly = false }],
        ["ro=false,cp=5", t => { t.readOnly = false; t.cursorPosition = 5 }],
        ["ro=true", t => { t.readOnly = false; root.on = false; t.readOnly = false; root.on = true; t.readOnly = true }],
        ["ro=false again", t => { t.readOnly = false; t.readOnly = false }]
    ]
    function run() {
        var ts_ = [["TI", ti, "hello world"], ["TM", tm, "hello-world"], ["TS", tm, "ab"], ["TE", te, "hello world"]]
        for (var a = 0; a < ts_.length; a++)
            for (var p = 0; p < pres.length; p++)
                for (var o = 0; o < ops.length; o++) {
                    var t = ts_[a][1]
                    on = false
                    t.readOnly = false
                    t.text = ""
                    t.text = ts_[a][2]
                    t.readOnly = true
                    pres[p][1](t)
                    log += ts_[a][0] + " " + pres[p][0] + " " + ops[o][0] + " " + st(t) + " ->"
                    on = true
                    ops[o][1](t)
                    on = false
                    log += " after" + st(t) + "\n"
                }
    }
    Component.onCompleted: { run(); console.log("LOG<" + log + ">LOG"); Qt.quit() }
}
