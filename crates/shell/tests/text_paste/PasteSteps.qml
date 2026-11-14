// paste() of TextEdit and TextInput called from a script: the signals it sends, what their
// handlers see and the state it leaves, for each text on the clipboard. A step is before(k),
// the clipboard set to `clip`, run(k), after(k). all() runs every step in one script, where
// after(k) reads what paste() left inside the call: paste.qml in Qt, keys.rs
// (script_paste_matches_qt) here. keys.rs also runs the steps with the text coming after the
// script (script_paste_after_the_script_matches_qt).
import QtQuick
Item {
    id: root
    property string log: ""
    property bool on: false
    // The clipboard's text for the current step; "" only before the first copy, as Qt has no
    // way to empty the clipboard from QML.
    property string clip: ""
    function esc(s) { return s.replace(/\n/g, "\\n") }
    function st(t) { return "(" + t.length + "|" + esc(t.text) + "|" + t.cursorPosition + "," + t.selectionStart + "," + t.selectionEnd + "|" + t.selectedText.length + ")" }
    function ev(n, t) { if (on) log += " " + n + st(t) }
    TextEdit {
        id: te
        onTextChanged: root.ev("text", te)
        onCursorPositionChanged: root.ev("cp", te)
        onSelectionStartChanged: root.ev("ss", te)
        onSelectionEndChanged: root.ev("se", te)
        onSelectedTextChanged: root.ev("st", te)
    }
    component Input: TextInput {
        id: i
        onTextChanged: root.ev("text", i)
        onTextEdited: root.ev("edited", i)
        onCursorPositionChanged: root.ev("cp", i)
        onSelectionStartChanged: root.ev("ss", i)
        onSelectionEndChanged: root.ev("se", i)
        onSelectedTextChanged: root.ev("st", i)
    }
    Input { id: ti }
    Input { id: tl; maximumLength: 13 }
    Input { id: tm; inputMask: "AAAAA-AAAAA;_" }
    Input { id: tv; validator: RegularExpressionValidator { regularExpression: /[a-z ]*/ } }
    readonly property var targets: [["te", te, "hello world"], ["ti", ti, "hello world"], ["tl", tl, "hello world"], ["tm", tm, "hello-world"], ["tv", tv, "hello world"]]
    readonly property var clips: ["", "XY", "a\nb", "молоко 2%", "\ud83d\ude00z", "xy"]
    readonly property var pres: [
        ["sel(2,4)", t => t.select(2, 4)],
        ["sel(4,2)", t => t.select(4, 2)],
        ["nosel@5", t => { t.cursorPosition = 5 }],
        ["nosel@0", t => { t.cursorPosition = 0 }],
        ["nosel@11", t => { t.cursorPosition = 11 }],
        ["all", t => t.selectAll()],
        ["ro sel(2,4)", t => { t.readOnly = true; t.select(2, 4) }],
        ["ro nosel@5", t => { t.readOnly = true; t.cursorPosition = 5 }],
        ["password nosel@5", t => { if (t !== te) t.echoMode = TextInput.Password; t.cursorPosition = 5 }]
    ]
    readonly property int count: clips.length * targets.length * pres.length * 2
    // Step k: the clipboard text changes slowest, so "" comes before any copy.
    function parts(k) {
        var read = k % 2; k = (k - read) / 2
        var p = k % pres.length; k = (k - p) / pres.length
        var a = k % targets.length; k = (k - a) / targets.length
        return [k, a, p, read]
    }
    function before(k) {
        var s = parts(k), a = targets[s[1]], t = a[1], p = pres[s[2]]
        clip = clips[s[0]]
        t.readOnly = false
        if (t !== te) t.echoMode = TextInput.Normal
        t.text = a[2]; p[1](t)
        var r = ""
        if (s[3] === 1) r = t.text
        log += a[0] + " " + p[0] + " clip=" + esc(clip) + " read=" + s[3] + " " + st(t) + " ->"
        on = true
    }
    function run(k) {
        try { targets[parts(k)[1]][1].paste() } catch (e) { log += " THROW " + e }
    }
    function after(k) {
        on = false
        log += " after" + st(targets[parts(k)[1]][1]) + "\n"
    }
    // Every step in one script; `copy` puts `clip` on the clipboard where one has to.
    function all(copy) {
        for (var k = 0; k < count; k++) {
            before(k)
            if (copy) copy()
            run(k)
            after(k)
        }
    }
}
