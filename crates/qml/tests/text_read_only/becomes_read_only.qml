// `readOnly = true` on TextInput (plain, masked in whole and in part) and TextEdit: the cursor
// goes to the end and the selection is dropped, as with `readOnly = false`.
import QtQuick
Item {
    id: root
    property string log: ""
    function st(t) { return "(" + t.text + "|" + t.cursorPosition + "," + t.selectionStart + "," + t.selectionEnd + "|" + t.selectedText + ")" }
    property bool on: false
    function ev(n, t) { if (on) log += " " + n + st(t) }
    component Input: TextInput {
        id: i
        onTextChanged: root.ev("text", i)
        onCursorPositionChanged: root.ev("cp", i)
        onSelectionStartChanged: root.ev("ss", i)
        onSelectionEndChanged: root.ev("se", i)
        onSelectedTextChanged: root.ev("st", i)
        onReadOnlyChanged: root.ev("ro", i)
    }
    Input { id: ti }
    Input { id: tm; inputMask: "AAAAA-AAAAA;_" }
    TextEdit { id: te
        onCursorPositionChanged: root.ev("cp", te)
        onSelectionStartChanged: root.ev("ss", te)
        onSelectionEndChanged: root.ev("se", te)
        onSelectedTextChanged: root.ev("st", te)
        onReadOnlyChanged: root.ev("ro", te)
    }
    Component.onCompleted: {
        var ts_ = [["TI", ti, "hello world"], ["TM", tm, "hello-world"], ["TS", tm, "ab"], ["TE", te, "hello world"]]
        var pres = [["sel(1,7)", t => t.select(1, 7)], ["sel(7,1)", t => t.select(7, 1)], ["cp=2", t => { t.cursorPosition = 2 }], ["end", t => { t.cursorPosition = t.length }], ["all", t => t.selectAll()]]
        for (var a = 0; a < ts_.length; a++) for (var p = 0; p < pres.length; p++) {
            var t = ts_[a][1]
            on = false; t.readOnly = false; t.text = ""; t.text = ts_[a][2]
            pres[p][1](t)
            log += ts_[a][0] + " " + pres[p][0] + " ro=true " + st(t) + " ->"
            on = true; t.readOnly = true; on = false
            log += " after" + st(t) + "\n"
        }
        console.log("LOG<" + log + ">LOG"); Qt.quit()
    }
}
