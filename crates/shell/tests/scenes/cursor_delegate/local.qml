import QtQuick
import QtQuick.Controls
Item {
    id: c
    width: 400; height: 300
    property string out
    function n(v) { return v.toFixed(2) }
    // The colour as a colour property holds it.
    property color held
    function shown(v) { held = v; return held }
    component Cur: Rectangle { objectName: "cur"; width: 2; color: "red"; property int tag: 7 }
    component Wide: Cur { width: 4; tag: 9 }
    function row(name, ed) {
        var s = " | " + name + " pos=" + ed.cursorPosition
        for (var i = 0; i < ed.children.length; i++) {
            var k = ed.children[i]
            if (k.objectName === "cur")
                s += " cur " + n(k.x) + " " + n(k.y) + " " + n(k.width) + " " + n(k.height) + " tag " + k.tag + " " + shown(k.color)
        }
        return s
    }
    function snap(tag) { out += tag + ":" + row("input", a) + row("edit", b) + row("field", f) + row("area", g) + "\n" }
    Component { id: wide; Wide { color: "blue" } }
    TextInput {
        id: a
        width: 180; text: "hello world"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Cur {}
    }
    TextEdit {
        id: b
        y: 40; width: 180; text: "hello\nworld"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Wide {}
    }
    TextField {
        id: f
        y: 100; width: 180; padding: 10; text: "hello world"; font.pixelSize: 16; focus: true
        cursorDelegate: Cur { color: "green" }
    }
    TextArea {
        id: g
        y: 160; width: 180; padding: 10; text: "hello\nworld"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: wide
    }
    property int steps: 3
    function step(k) {
        switch (k) {
        case 1: a.cursorDelegate = wide; f.cursorDelegate = wide; break
        }
        snap(k)
    }
}
