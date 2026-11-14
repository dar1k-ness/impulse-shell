// TextArea: its cursor item through focus, moves, edits, cursorVisible, another delegate, none, readOnly, alignment and font
import QtQuick
import QtQuick.Controls
Item {
    id: c
    width: 300; height: 200
    // The log: a line per step, and the delegates' own lines (created, destroyed) as they come.
    property string out
    function note(s) { out += "  " + s + "\n" }
    function n(v) { return v.toFixed(2) }
    // The editor's cursor state and every cursor item it holds.
    function snap(tag) {
        var r = ed.cursorRectangle
        var items = []
        for (var i = 0; i < ed.children.length; i++) {
            var k = ed.children[i]
            if (k.objectName.indexOf("cur") === 0)
                items.push(k.objectName + " " + n(k.x) + " " + n(k.y) + " " + n(k.width) + " " + n(k.height) + (k.visible ? " shown" : " hidden"))
        }
        out += tag + ": pos=" + ed.cursorPosition + " cursorVisible=" + ed.cursorVisible + " activeFocus=" + ed.activeFocus
            + " delegate=" + (ed.cursorDelegate === null ? "null" : ed.cursorDelegate === alt ? "alt" : "own")
            + " rect=" + n(r.x) + " " + n(r.y) + " " + n(r.width) + " " + n(r.height)
            + " items=[" + items.join("; ") + "]\n"
    }
    Item { id: away }
    Component {
        id: alt
        Rectangle {
            objectName: "curB"; width: 5; color: "blue"
            Component.onCompleted: c.note("created curB")
            Component.onDestruction: c.note("destroyed curB")
        }
    }
    TextArea {
        id: ed
        x: 10; y: 20; width: 200; padding: 10
        text: "hello world"
        font.pixelSize: 16
        cursorDelegate: Rectangle {
            objectName: "curA"; width: 3; color: "red"
            Component.onCompleted: c.note("created curA" + (parent === ed ? " in the editor" : " elsewhere") + " at " + c.n(x) + " " + c.n(y) + " height " + c.n(height))
            Component.onDestruction: c.note("destroyed curA")
        }
    }
    property int steps: 27
    function step(k) {
        snap(k)
        switch (k) {
        case 1: ed.forceActiveFocus(); break
        case 2: ed.cursorPosition = 3; break
        case 3: ed.text = "hi"; break
        case 4: ed.insert(2, " there"); break
        case 5: ed.select(1, 4); break
        case 6: away.forceActiveFocus(); break
        case 7: ed.cursorPosition = 1; break
        case 8: ed.cursorVisible = true; break
        case 9: ed.cursorVisible = false; break
        case 10: ed.cursorDelegate = alt; break
        case 11: ed.forceActiveFocus(); break
        case 12: ed.cursorDelegate = null; break
        case 13: ed.cursorDelegate = alt; break
        case 14: ed.readOnly = true; break
        case 15: ed.readOnly = false; break
        case 16: ed.horizontalAlignment = TextInput.AlignRight; break
        case 17: ed.font.pixelSize = 24; break
        case 18: away.forceActiveFocus(); break
        case 19: ed.cursorVisible = true; break
        case 20: ed.readOnly = true; break
        case 21: ed.cursorVisible = true; break
        case 22: ed.readOnly = false; break
        case 23: ed.readOnly = true; break
        case 24: ed.forceActiveFocus(); break
        case 25: ed.readOnly = false; break
        }
    }
}
