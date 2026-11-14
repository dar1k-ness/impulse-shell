// What cut() and copy() of TextEdit and TextInput leave on the clipboard: every step starts with
// "-" there and ends with a paste() of it into `sink`.
import QtQuick
Item {
    id: root
    property string log: ""
    TextEdit { id: te }
    TextInput { id: ti }
    TextInput { id: h }
    TextInput { id: sink }
    function st(t) { return "(" + t.text + "|" + t.cursorPosition + "," + t.selectionStart + "," + t.selectionEnd + ")" }
    function step(name, t, pre, f) {
        h.text = "-"; h.selectAll(); h.copy()
        t.readOnly = false
        if (t === ti) t.echoMode = TextInput.Normal
        t.text = "hello world"; pre(t)
        f(t)
        sink.text = ""; sink.paste()
        log += name + " -> " + st(t) + " clip=" + sink.text + "\n"
    }
    Component.onCompleted: {
        var pres = [
            ["sel(2,4)", t => t.select(2, 4)],
            ["sel(4,2)", t => t.select(4, 2)],
            ["nosel@5", t => { t.cursorPosition = 5 }],
            ["all", t => t.selectAll()],
            ["ro sel(2,4)", t => { t.readOnly = true; t.select(2, 4) }],
            ["password sel(2,4)", t => { if (t === ti) t.echoMode = TextInput.Password; t.select(2, 4) }],
            ["noecho sel(2,4)", t => { if (t === ti) t.echoMode = TextInput.NoEcho; t.select(2, 4) }]
        ]
        var ops = [["cut", t => t.cut()], ["copy", t => t.copy()]]
        var ts = [["te", te], ["ti", ti]]
        for (var a = 0; a < 2; a++)
            for (var p = 0; p < pres.length; p++)
                for (var i = 0; i < ops.length; i++)
                    step(ts[a][0] + " " + pres[p][0] + " " + ops[i][0], ts[a][1], pres[p][1], ops[i][1])
        console.log("LOG<" + log + ">LOG"); Qt.quit()
    }
}
