import QtQuick
Item {
    id: root
    property string log: ""
    function st(t) { return t.text + "|" + t.cursorPosition + "|" + t.selectionStart + "|" + t.selectionEnd + "|" + t.selectedText + "|" + t.length }
    // Start states: selection 2..4 with the cursor at its end, at its start, no selection with
    // the cursor inside the text, and at its end. `select(0, 0)` changes nothing in a TextEdit
    // whose text was just assigned (Qt puts the cursor at 0 there).
    function pre(t, k) {
        t.text = ""; t.text = "hello world"; t.select(0, 0)
        if (k === 0) t.select(2, 4)
        else if (k === 1) t.select(4, 2)
        else if (k === 2) t.cursorPosition = 5
        else t.cursorPosition = 11
    }
    // [label, call, TextInput only]
    property var ops: [
        ["select(20,30)", t => t.select(20, 30)],
        ["select(-5,3)", t => t.select(-5, 3)],
        ["select(3,99)", t => t.select(3, 99)],
        ["select(3,-1)", t => t.select(3, -1)],
        ["select(12,0)", t => t.select(12, 0)],
        ["select(6,1)", t => t.select(6, 1)],
        ["select(1,6)", t => t.select(1, 6)],
        ["select(2,4)", t => t.select(2, 4)],
        ["select(4,2)", t => t.select(4, 2)],
        ["select(0,11)", t => t.select(0, 11)],
        ["select(11,0)", t => t.select(11, 0)],
        ["select(11,11)", t => t.select(11, 11)],
        ["select(3,3)", t => t.select(3, 3)],
        ["select(1.7,3.2)", t => t.select(1.7, 3.2)],
        ["insert(-1,Y)", t => t.insert(-1, "Y")],
        ["insert(99,X)", t => t.insert(99, "X")],
        ["insert(12,X)", t => t.insert(12, "X")],
        ["insert(0,5)", t => t.insert(0, 5)],
        ["insert(0,ab)", t => t.insert(0, "ab")],
        ["insert(2,A)", t => t.insert(2, "A")],
        ["insert(3,ZZ)", t => t.insert(3, "ZZ")],
        ["insert(4,B)", t => t.insert(4, "B")],
        ["insert(5,Z)", t => t.insert(5, "Z")],
        ["insert(8,Q)", t => t.insert(8, "Q")],
        ["insert(11,!)", t => t.insert(11, "!")],
        ["insert(3,)", t => t.insert(3, "")],
        ["insert(length,!)", t => t.insert(t.length, "!")],
        ["remove(6,9)", t => t.remove(6, 9)],
        ["remove(5,99)", t => t.remove(5, 99)],
        ["remove(8,3)", t => t.remove(8, 3)],
        ["remove(50,60)", t => t.remove(50, 60)],
        ["remove(-3,3)", t => t.remove(-3, 3)],
        ["remove(0,1)", t => t.remove(0, 1)],
        ["remove(0,2)", t => t.remove(0, 2)],
        ["remove(1,5)", t => t.remove(1, 5)],
        ["remove(2,4)", t => t.remove(2, 4)],
        ["remove(3,4)", t => t.remove(3, 4)],
        ["remove(4,7)", t => t.remove(4, 7)],
        ["remove(0,11)", t => t.remove(0, 11)],
        ["remove(4,4)", t => t.remove(4, 4)],
        ["remove(9,11)", t => t.remove(9, 11)],
        ["getText(3,1)", t => t.getText(3, 1)],
        ["getText(2,5)", t => t.getText(2, 5)],
        ["getText(-2,99)", t => t.getText(-2, 99)],
        ["selectAll", t => t.selectAll()],
        ["deselect", t => t.deselect()],
        ["clear", t => t.clear()],
        ["cursorPosition=7", t => { t.cursorPosition = 7 }],
        ["cursorPosition=3", t => { t.cursorPosition = 3 }],
        ["text=xy", t => { t.text = "xy" }, true],
        ["text=same", t => { t.text = "hello world" }, true]
    ]
    property bool on: false
    function ev(n, t) { if (on) log += " " + n + "(" + st(t) + ")" }
    TextInput {
        id: ti
        onLengthChanged: root.ev("len", ti)
        onTextChanged: root.ev("text", ti)
        onTextEdited: root.ev("edited", ti)
        onCursorPositionChanged: root.ev("cp", ti)
        onSelectionStartChanged: root.ev("ss", ti)
        onSelectionEndChanged: root.ev("se", ti)
        onSelectedTextChanged: root.ev("st", ti)
    }
    TextEdit {
        id: te
        onLengthChanged: root.ev("len", te)
        onTextChanged: root.ev("text", te)
        onCursorPositionChanged: root.ev("cp", te)
        onSelectionStartChanged: root.ev("ss", te)
        onSelectionEndChanged: root.ev("se", te)
        onSelectedTextChanged: root.ev("st", te)
    }
    function run() {
        var ts = [["TI", ti], ["TE", te]]
        for (var a = 0; a < ts.length; a++)
            for (var k = 0; k < 4; k++)
                for (var i = 0; i < ops.length; i++) {
                    if (a === 1 && ops[i][2]) continue
                    var t = ts[a][1]
                    pre(t, k)
                    log += ts[a][0] + " " + k + " " + ops[i][0] + " ->"
                    on = true
                    try { ops[i][1](t) } catch (e) { log += " !" + e }
                    on = false
                    log += "\n"
                }
    }
    Component.onCompleted: { run(); console.log("LOG<" + log + ">LOG"); Qt.quit() }
}
