// The cursor item right after a change, read by the script that made the change
import QtQuick
import QtQuick.Controls
Item {
    id: c
    width: 400; height: 300
    property string out
    property var field: f
    function n(v) { return v.toFixed(2) }
    function row(name, ed) {
        var s = " | " + name + " pos=" + ed.cursorPosition
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
    }
    TextEdit {
        id: b
        y: 40; width: 180; text: "hello\nworld"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" }
    }
    TextField {
        id: f
        y: 100; width: 180; padding: 10; text: "hello world"; font.pixelSize: 16; focus: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" }
    }
    TextArea {
        id: g
        y: 160; width: 180; padding: 10; text: "hello\nworld"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "cur"; width: 2; color: "red" }
    }
    property int steps: 8
    function step(k) {
        switch (k) {
        case 1: a.cursorPosition = 3; b.cursorPosition = 3; f.cursorPosition = 3; g.cursorPosition = 3; break
        case 2: a.cursorPosition = 0; b.cursorPosition = 0; f.cursorPosition = 0; g.cursorPosition = 0; break
        case 3: a.insert(0, "ab"); b.insert(0, "ab"); f.insert(0, "ab"); g.insert(0, "ab"); break
        case 4: a.cursorPosition = 8; b.cursorPosition = 8; f.cursorPosition = 8; g.cursorPosition = 8; break
        case 5: a.remove(0, 4); b.remove(0, 4); f.remove(0, 4); g.remove(0, 4); break
        case 6: a.text = "WIDE wide"; b.text = "WIDE\nwide"; f.text = "WIDE wide"; g.text = "WIDE\nwide"; break
        case 7: a.select(1, 4); b.select(1, 4); f.select(1, 4); g.select(1, 4); break
        }
        snap(k)
    }
}
