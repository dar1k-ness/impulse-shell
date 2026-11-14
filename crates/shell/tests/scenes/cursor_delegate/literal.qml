import QtQuick
import QtQuick.Controls
Item {
    id: c
    width: 400; height: 400
    property string out
    function n(v) { return v.toFixed(2) }
    function row(name, ed) {
        var s = " | " + name + " pos=" + ed.cursorPosition + " sel=" + ed.selectionStart + "-" + ed.selectionEnd + " rect " + n(ed.cursorRectangle.x) + " " + n(ed.cursorRectangle.y)
        for (var i = 0; i < ed.children.length; i++) {
            var k = ed.children[i]
            if (k.objectName === "cur")
                s += " cur " + n(k.x) + " " + n(k.y) + " " + n(k.height)
        }
        return s
    }
    function snap(tag) {
        out += tag + ":" + row("input", a) + row("edit", b) + row("field", f) + row("area", g) + "\n"
        out += tag + "':" + row("input", a2) + row("edit", b2) + row("field", f2) + row("area", g2) + "\n"
        out += tag + "\":" + row("input", a3) + row("edit", b3) + row("field", f3) + row("area", g3) + "\n"
    }
    // text, then cursorPosition
    TextInput { id: a; width: 180; text: "hello world"; cursorPosition: 3; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    TextEdit { id: b; y: 30; width: 180; text: "hello\nworld"; cursorPosition: 3; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    TextField { id: f; y: 80; width: 180; padding: 10; text: "hello world"; cursorPosition: 3; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    TextArea { id: g; y: 130; width: 180; padding: 10; text: "hello\nworld"; cursorPosition: 3; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    // cursorPosition, then text
    TextInput { id: a2; x: 200; width: 180; cursorPosition: 3; text: "hello world"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    TextEdit { id: b2; x: 200; y: 30; width: 180; cursorPosition: 3; text: "hello\nworld"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    TextField { id: f2; x: 200; y: 80; width: 180; padding: 10; cursorPosition: 3; text: "hello world"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    TextArea { id: g2; x: 200; y: 130; width: 180; padding: 10; cursorPosition: 3; text: "hello\nworld"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    // text a binding, cursorPosition a literal; and cursorPosition a binding, text a literal
    property string t: "hello world"
    property int p: 3
    TextInput { id: a3; y: 200; width: 180; text: c.t; cursorPosition: 3; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    TextEdit { id: b3; y: 230; width: 180; text: "hello\nworld"; cursorPosition: c.p; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    TextField { id: f3; y: 280; width: 180; padding: 10; text: c.t; cursorPosition: c.p; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    TextArea { id: g3; y: 330; width: 180; padding: 10; text: "hello\nworld"; cursorPosition: c.p; selectByMouse: true; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" } }
    Component.onCompleted: snap("done")
    property int steps: 3
    function step(k) {
        switch (k) {
        case 1: c.t = "HELLO"; break
        case 2: c.p = 1; break
        }
        snap(k)
    }
}
