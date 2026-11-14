// A delegate that sets its own x, y and height: as numbers, and as bindings
import QtQuick
Item {
    id: c
    width: 300; height: 200
    property string out
    function n(v) { return v.toFixed(2) }
    function row(ed) {
        var s = ""
        for (var i = 0; i < ed.children.length; i++) {
            var k = ed.children[i]
            if (k.objectName.indexOf("cur") === 0)
                s += " " + k.objectName + " " + n(k.x) + " " + n(k.y) + " " + n(k.width) + " " + n(k.height)
        }
        return s
    }
    function snap(tag) { out += tag + ":" + row(a) + row(b) + row(d) + "\n" }
    TextInput {
        id: a
        width: 200; text: "hello"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "curNumbers"; x: 2; y: 3; width: 4; height: 10 }
    }
    TextInput {
        id: b
        y: 40; width: 200; text: "hello"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "curBound"; x: b.width / 2; y: b.height / 2; width: 4; height: b.height / 2 }
    }
    TextEdit {
        id: d
        y: 80; width: 200; text: "hello\nworld"; font.pixelSize: 16; cursorVisible: true
        cursorDelegate: Rectangle { objectName: "curEdit"; x: 2; y: 3; width: 4; height: d.width / 20 }
    }
    property int steps: 6
    function step(k) {
        snap(k)
        switch (k) {
        case 1: a.cursorPosition = 1; b.cursorPosition = 1; d.cursorPosition = 7; break
        case 2: a.width = 100; b.width = 100; d.width = 100; break
        case 3: a.font.pixelSize = 20; b.font.pixelSize = 20; d.font.pixelSize = 20; break
        case 4: a.cursorPosition = 2; b.cursorPosition = 2; d.cursorPosition = 2; break
        }
    }
}
