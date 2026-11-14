// A value type read from a property and used after the property changed shows the new value
// (Qt 6.12), also when only the expression that read it knows the object: one a function
// returns, one indexed by a key whose expression writes the property. Two reads of a property
// are objects of their own to a write of a member script adds, and equal to `===`, a WeakSet
// and a Map.
// qt: 9|7|1|1|1|1|6|true|true|true|10|10|true|true|true|1||x,y||12|5,5|[{"width":10,"height":4},{"width":10,"height":4}]
import QtQuick
Item {
    id: root
    property string result
    property point at: Qt.point(1, 2)
    property color col: "red"
    property size sz: Qt.size(3, 4)
    property int n: 0
    onAtChanged: n += 1
    Rectangle { id: src; property point at: Qt.point(1, 2); color: "red" }
    function taken() { var q = src.at; src.at = Qt.point(9, 9); return q }
    function colour() { var q = src.color; src.color = "blue"; return q }
    function same() { var q = root.at; root.at = Qt.point(1, 2); return q }
    Component.onCompleted: {
        var l = []
        l.push(taken().x)
        l.push(src.at[(src.at = Qt.point(7, 7), "x")])
        l.push(colour().b)
        l.push(src.color[(src.color = "#00ff00", "g")])
        l.push(same().x, n)
        l.push(root.at.x + (root.at = Qt.point(5, 5), root.at.x))
        l.push(root.at === root.at, root.at == root.at, root.col === root.col)
        var a = root.sz, b = root.sz; a.width = 10; l.push(b.width, root.sz.width, a === b)
        var w = new WeakSet(); w.add(root.at); l.push(w.has(root.at))
        var m = new Map(); m.set(root.col, 1); l.push(m.has(root.col), m.size)
        var k = root.at; k.zz = 5; l.push(root.at.zz, Object.keys(root.at).join(","))
        var f = root.col; f.zz = 5; l.push(root.col.zz, Object.keys(root.col).length)
        l.push([root.at, root.at].map(p => p.x).join(","), JSON.stringify([root.sz, root.sz]))
        result = l.join("|")
    }
}
