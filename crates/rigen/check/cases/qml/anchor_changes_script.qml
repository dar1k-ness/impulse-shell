// QQuickAnchorSet keeps the expressions of an AnchorChanges: each of its anchors reads as a
// QQmlScriptString value, one nothing set too; two are equal when they hold the same script;
// a line written to one is refused.
// qt: object:QQmlScriptString() object:QQmlScriptString() object:QQmlScriptString() object:QQmlScriptString() object:QQmlScriptString() object:QQmlScriptString() object:QQmlScriptString() | object:QQuickAnchorSet(0x) | true true false true false false false false false false | true {} 0 [object QMLValueTypeWrapper] undefined:undefined | Error: Cannot assign QQuickAnchorLine to QQmlScriptString, Error: Cannot assign QObject* to QQmlScriptString, ok, object:QQmlScriptString(), object:QQmlScriptString()
import QtQuick
Item {
    id: root
    Item { id: a; width: 11; height: 13 }
    Item { id: b }
    states: State {
        name: "s"
        AnchorChanges { id: ac; target: b; anchors.left: a.right; anchors.top: undefined; anchors.bottom: a.right }
        AnchorChanges { id: ad; target: b; anchors.left: a.right }
    }
    readonly property var lines: ["left", "right", "top", "bottom", "horizontalCenter", "verticalCenter", "baseline"]
    function d(v) { return typeof v + ":" + String(v) }
    function t(f) { try { f(); return "ok" } catch (x) { return String(x) } }
    property string result
    Component.onCompleted: {
        var l = []
        l.push(lines.map(k => d(ac.anchors[k])).join(" "))
        l.push(d(ac.anchors).replace(/0x[0-9a-f]+/, "0x"))
        var v = ac.anchors.left
        v.x = 1
        l.push([ac.anchors.left == ac.anchors.left, ac.anchors.left === ac.anchors.left, ac.anchors.left == ac.anchors.right,
                ac.anchors.right == ac.anchors.baseline, ac.anchors.left == ac.anchors.bottom, ac.anchors.left == ad.anchors.left,
                ac.anchors.left == a.right, ac.anchors.right == a.anchors.right, ac.anchors.top == undefined, ac.anchors.right == null].join(" "))
        l.push([!!ac.anchors.right, JSON.stringify(v), Object.keys(v).length, Object.prototype.toString.call(v), d(v.x)].join(" "))
        l.push([t(() => { ac.anchors.left = a.left }), t(() => { ac.anchors.right = a }), t(() => { ac.anchors.right = null }), d(ac.anchors.left), d(ac.anchors.right)].join(", "))
        result = l.join(" | ")
    }
}
