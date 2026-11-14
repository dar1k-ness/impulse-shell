import QtQuick
Item {
    id: root
    property string log: ""
    property bool on: false
    function ev(n, t) { if (on) log += " " + n + "(" + t.length + "|" + t.text + "|" + t.length + ")" }
    function evl(n, t) { if (on) log += " " + n + "(" + t.length + ")" }
    TextEdit {
        id: te
        text: "hello world"
        onTextChanged: root.ev("text", te)
        onCursorPositionChanged: root.ev("cp", te)
        onSelectionStartChanged: root.ev("ss", te)
        onSelectionEndChanged: root.ev("se", te)
        onSelectedTextChanged: root.ev("st", te)
    }
    TextEdit {
        id: tl
        text: "hello world"
        onTextChanged: root.evl("text", tl)
        onCursorPositionChanged: root.evl("cp", tl)
        onSelectionStartChanged: root.evl("ss", tl)
        onSelectionEndChanged: root.evl("se", tl)
        onSelectedTextChanged: root.evl("st", tl)
    }
    function step(name, t, read, f) {
        t.text = "hello world"; t.select(2, 4)
        var r = ""
        if (read === 1) r = t.text
        if (read === 2) r = "" + t.length
        log += name + " read=" + read + " ->"
        on = true; f(t); on = false
        log += " after(" + t.length + "|" + t.text + ")\n"
    }
    Component.onCompleted: {
        var ops = [
            ["insert(2,X)", t => t.insert(2, "X")],
            ["insert(length,!)", t => t.insert(t.length, "!")],
            ["remove(0,3)", t => t.remove(0, 3)],
            ["remove(8,3)", t => t.remove(8, 3)],
            ["clear", t => t.clear()],
            ["select(1,3)", t => t.select(1, 3)]
        ]
        var ts = [["te", te], ["tl", tl]]
        for (var a = 0; a < 2; a++)
            for (var i = 0; i < ops.length; i++)
                for (var r = 0; r < 3; r++)
                    step(ts[a][0] + " " + ops[i][0], ts[a][1], r, ops[i][1])
        console.log("LOG<" + log + ">LOG"); Qt.quit()
    }
}
