// Item.transform is a QML list of transforms, as in Qt 6.12: one transform reads as a list of
// one, an array or a single transform replaces the list, `push` and `length = 0` change it,
// `null` empties it, anything that is not a transform is left out, `undefined` throws, and one
// transform object in the lists of two items moves both.
// qt: object 1 2 0 1 2 true true true undefined undefined | 2 1 2 0 0 0 2 0 2 Error: Cannot assign [undefined] to QQmlListProperty<QQuickTransform> 2 2 false 0 2 0 | 40 41 101 50 1
import QtQuick
Item {
    id: root
    property string result: ""
    Translate { id: t; x: 3 }
    Scale { id: s }
    Item { id: a; transform: Translate { id: at; x: 3 } }
    Item { id: b; transform: [ Translate { x: 3 }, Scale { id: bs } ] }
    Item { id: c }
    Item { id: d; transform: t }
    Item { id: e; x: 1; transform: [t, s] }
    function n(it) { return it.transform.length }
    function tryset(it, v) { try { it.transform = v } catch (x) { result += " " + x } result += " " + n(it) }
    Component.onCompleted: {
        result += typeof a.transform + " " + n(a) + " " + n(b) + " " + n(c) + " " + n(d) + " " + n(e) + " " + (a.transform[0] === at) + " " + (b.transform[1] === bs) + " " + (d.transform[0] === e.transform[0]) + " " + at.parent + " " + a.transform[1]
        result += " |"
        tryset(c, [t, s]); tryset(c, t); c.transform.push(s); result += " " + n(c)
        c.transform.length = 0; result += " " + n(c)
        tryset(c, 5); tryset(c, [c]); tryset(c, [t, s]); tryset(c, null); tryset(c, [t, s]); tryset(c, undefined)
        tryset(c, [t, s, t]); result += " " + (c.transform[2] === t)
        tryset(c, "x"); tryset(c, [t, 5, s]); tryset(c, c)
        result += " |"
        // One transform object in the lists of two items moves both.
        t.x = 40
        result += " " + d.mapToItem(root, 0, 0).x + " " + e.mapToItem(root, 0, 0).x
        s.xScale = 2
        result += " " + e.mapToItem(root, 10, 0).x
        e.transform = []
        t.x = 50
        result += " " + d.mapToItem(root, 0, 0).x + " " + e.mapToItem(root, 0, 0).x
    }
}
