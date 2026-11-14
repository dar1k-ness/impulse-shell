import QtQuick
Item {
    id: root
    property string log: ""
    property bool on: false
    function ev(s) { if (on) log += s + "\n" }
    TextInput { id: a; text: "hello"; onTextChanged: root.ev("a text " + length); onLengthChanged: root.ev("a len " + length) }
    TextInput { id: b; text: "hello"; onLengthChanged: root.ev("b len " + length); onTextChanged: root.ev("b text " + length) }
    TextEdit { id: c; text: "hello"; onTextChanged: root.ev("c text " + length); onLengthChanged: root.ev("c len " + length) }
    TextEdit { id: d; text: "hello"; onLengthChanged: root.ev("d len " + length); onTextChanged: root.ev("d text " + length) }
    TextInput { id: e; text: "hello" }
    Connections { target: e; function onTextChanged() { root.ev("e text " + e.length) } function onLengthChanged() { root.ev("e len " + e.length) } }
    TextInput { id: f; text: "hello" }
    // A binding on `length` follows the text.
    property int fl: f.length
    TextInput { id: g; text: "hello"; onTextEdited: root.ev("g edited"); onLengthChanged: root.ev("g len " + length) }
    TextEdit { id: h; text: "hello"; onLengthChanged: root.ev("h len " + length) }
    // `length` counts UTF-16 code units.
    TextInput { id: u; text: "a\ud83d\ude00\u00e9" }
    TextEdit { id: v; text: "a\ud83d\ude00\u00e9" }
    function run() {
        on = true
        ev("init " + [a.length, b.length, c.length, d.length, e.length, f.length, fl, u.length, v.length].join(" "))
        var ts = [a, b, c, d, e, f, g, h]
        for (var i = 0; i < ts.length; i++) {
            var t = ts[i]
            ev("-- " + i)
            t.text = "hello world"; ev(t.length + " " + fl)
            t.insert(t.length, "!"); ev(t.text + " " + t.length + " " + fl)
            t.text = "HELLO WORLD!"; ev(t.length + " " + fl)
            t.remove(0, 1); ev(t.text + " " + t.length + " " + fl)
            t.insert(0, ""); ev(t.text + " " + t.length + " " + fl)
            t.clear(); ev(t.text + " " + t.length + " " + fl)
            t.clear(); ev(t.text + " " + t.length + " " + fl)
        }
        u.insert(u.length, "\ud83d\ude00"); ev("u " + u.length)
        v.insert(v.length, "\ud83d\ude00"); ev("v " + v.length)
        u.remove(1, 3); ev("u " + u.length + " " + (u.text === "a\u00e9\ud83d\ude00"))
        v.remove(1, 3); ev("v " + v.length + " " + (v.text === "a\u00e9\ud83d\ude00"))
    }
    Component.onCompleted: { run(); console.log("LOG<" + log + ">LOG"); Qt.quit() }
}
