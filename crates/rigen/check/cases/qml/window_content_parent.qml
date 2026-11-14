// The items declared in a Window are children of its `contentItem`, not of the window: that
// is what they read as `parent`, it has the window's size, and no parent of its own.
// qt: a>content>null content>null true undefined | undefined,0,0,true,true | 2 a c QQuickRootItem | content content content object content 3 b
import QtQuick
Item {
    id: root
    property string result: ""
    Window {
        id: win
        width: 320
        height: 200
        Item {
            id: a
            anchors.fill: parent
            property real pw: parent.width
            property real ph: parent.height
            property string scr: typeof parent.screen
            Item { id: b }
        }
        Rectangle {
            id: c
            anchors.centerIn: parent
        }
    }
    function nm(it) {
        return it === a ? "a" : it === b ? "b" : it === c ? "c" : it === win ? "win"
            : it === win.contentItem ? "content" : it === root ? "root" : it === null ? "null" : typeof it
    }
    function chain(item) {
        var out = []
        for (var it = item.parent, n = 0; it && n < 8; it = it.parent, n++)
            out.push(nm(it))
        return out.join(">") + ">" + nm(it)
    }
    Component.onCompleted: {
        var ci = win.contentItem
        var out = chain(b) + " " + chain(c) + " " + (a.parent === ci) + " " + nm(win.parent)
        // The contentItem gets the window's size when the window is shown: equalities only.
        out += " | " + [a.scr, ci.x, ci.y, a.pw === ci.width, a.ph === ci.height]
        out += " | " + ci.children.length + " " + nm(ci.children[0]) + " " + nm(ci.children[1])
        out += " " + ci.toString().replace(/\(.*/, "")
        // An anchor to `parent` there is an anchor to the contentItem.
        out += " | " + nm(a.anchors.fill) + " " + nm(c.anchors.centerIn)
        c.anchors.centerIn = undefined
        c.anchors.fill = ci
        out += " " + nm(c.anchors.fill) + " " + typeof ci.right
        b.parent = ci
        out += " " + nm(b.parent) + " " + ci.children.length + " " + nm(ci.children[2])
        result = out
    }
}
