// A QQuickAnchorLine is a value: two lines are equal, by `==` and by `===`, when they name the
// same line of the same item; an anchor nothing set, one a script reset and one whose binding
// gave `undefined` read the line of no item; a number, a string, an item or `null` written to
// an anchor is refused and leaves it; a field written to a line is dropped.
// qt: object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() | object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() | object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() | true true false false false true true | true true false true true true true | true true true false false false false | true {} 0 [object QMLValueTypeWrapper] true undefined:undefined undefined:undefined | Error: Cannot assign int to QQuickAnchorLine, Error: Cannot assign QString to QQuickAnchorLine, Error: Cannot assign QObject* to QQuickAnchorLine, Error: Cannot assign std::nullptr_t to QQuickAnchorLine, Error: Cannot assign bool to QQuickAnchorLine, true | object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() object:QQuickAnchorLine() true true false object:QQuickAnchorLine() | object:QQuickAnchorLine() true false true true
import QtQuick
Item {
    id: root
    property bool on: true
    property var kept: a.left
    Item { id: a; width: 11; height: 13 }
    Item { id: b; anchors.left: a.right; anchors.top: a.bottom }
    Text { id: c; anchors.baseline: a.baseline }
    Item { id: e; anchors.left: root.on ? a.right : undefined; anchors.verticalCenter: root.on ? a.verticalCenter : undefined }
    readonly property var lines: ["left", "right", "top", "bottom", "horizontalCenter", "verticalCenter", "baseline"]
    function d(v) { return typeof v + ":" + String(v) }
    function t(f) { try { f(); return "ok" } catch (x) { return String(x) } }
    property string result
    Component.onCompleted: {
        var l = []
        l.push(lines.map(k => d(a.anchors[k])).join(" "))
        l.push(lines.map(k => d(a[k])).join(" "))
        l.push([d(b.anchors.left), d(c.anchors.baseline), d(e.anchors.verticalCenter), d(kept)].join(" "))
        l.push([a.left == a.left, a.left === a.left, a.left != a.left, a.left == a.right, a.left === b.left, kept == a.left, kept === root.kept].join(" "))
        l.push([b.anchors.left == a.right, b.anchors.left === a.right, b.anchors.left == a.left, b.anchors.top === a.bottom,
                c.anchors.baseline == a.baseline, e.anchors.left === b.anchors.left, e.anchors.verticalCenter == a.verticalCenter].join(" "))
        l.push([a.anchors.left == a.anchors.right, a.anchors.left === a.anchors.top, a.anchors.left == b.anchors.right,
                a.anchors.left == b.anchors.left, a.anchors.left == undefined, a.anchors.left == null, a.anchors.left == a].join(" "))
        var v = b.anchors.left
        v.x = 1
        l.push([!!a.anchors.left, JSON.stringify(v), Object.keys(v).length, Object.prototype.toString.call(v), v instanceof Object, d(v.x), d(v.item)].join(" "))
        l.push([t(() => { b.anchors.left = 5 }), t(() => { b.anchors.left = "left" }), t(() => { b.anchors.left = a }),
                t(() => { b.anchors.left = null }), t(() => { b.anchors.left = true }), b.anchors.left == a.right].join(", "))
        b.anchors.left = undefined; c.anchors.baseline = undefined; on = false
        l.push([d(b.anchors.left), d(c.anchors.baseline), d(e.anchors.left), d(e.anchors.verticalCenter),
                b.anchors.left == a.anchors.left, b.anchors.left === e.anchors.left, b.anchors.left == a.right, d(b.anchors.top)].join(" "))
        b.anchors.left = a.left; on = true
        l.push([d(b.anchors.left), b.anchors.left === a.left, b.anchors.left == a.right, e.anchors.left === a.right, e.anchors.verticalCenter === a.verticalCenter].join(" "))
        result = l.join(" | ")
    }
}
