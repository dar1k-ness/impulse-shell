import QtQuick
import QtQuick.Controls
Item {
    id: c
    width: 400; height: 300
    property string out
    function n(v) { return v.toFixed(2) }
    function row(name, ed) {
        var r = ed.cursorRectangle
        var s = " | " + name + " pos=" + ed.cursorPosition + " rect " + n(r.x) + " " + n(r.y) + " " + n(r.width) + " " + n(r.height)
        for (var i = 0; i < ed.children.length; i++) {
            var k = ed.children[i]
            if (k.objectName === "cur")
                s += " cur " + n(k.x) + " " + n(k.y) + " " + n(k.height)
        }
        return s
    }
    function snap(tag) { out += tag + ":" + row("input", a) + row("edit", b) + row("field", f) + row("area", g) + "\n" }
    TextInput {
        id: a
        width: 180; text: "hello world"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" }
        Component.onCompleted: c.out += "input done" + c.row("input", a) + "\n"
    }
    TextEdit {
        id: b
        y: 40; width: 180; text: "hello\nworld"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" }
        Component.onCompleted: { cursorPosition = 8; c.out += "edit done" + c.row("edit", b) + "\n" }
    }
    TextField {
        id: f
        y: 100; width: 180; padding: 10; text: "hello world"; font.pixelSize: 16; focus: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" }
        Component.onCompleted: c.out += "field done" + c.row("field", f) + "\n"
    }
    TextArea {
        id: g
        y: 160; width: 180; padding: 10; text: "hello\nworld"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" }
        Component.onCompleted: { cursorPosition = 8; c.out += "area done" + c.row("area", g) + "\n" }
    }
    Component.onCompleted: snap("root done")
    property int steps: 2
    function step(k) { snap(k) }
}
