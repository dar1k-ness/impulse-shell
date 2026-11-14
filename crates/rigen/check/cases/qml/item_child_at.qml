// Item.childAt(x, y) as in Qt 6.12: the last child item, in the order of `children`, that is
// visible and contains the point mapped into it. `z`, `clip`, `opacity` and `enabled` do not
// matter; a child of a hidden item is hidden; `scale` and `rotation` of the child are undone.
// qt: a b b b b null null null op dis cl cl null null sc sc null sc null sc ro ro ro null null null tl tl null null null r0 r1 null a | cc cc null null | a null null null a null a null a | b a a hid function
import QtQuick
Item {
    id: root
    width: 200; height: 200
    property string result: ""
    Item { id: a; objectName: "a"; x: 0; y: 0; width: 50; height: 50; z: 5 }
    Item { id: b; objectName: "b"; x: 25; y: 25; width: 50; height: 50 }
    Item { id: hid; objectName: "hid"; x: 100; y: 0; width: 20; height: 20; visible: false }
    Item { id: hp; objectName: "hp"; x: 100; y: 30; width: 20; height: 20; visible: false
        Item { id: hc; objectName: "hc"; width: 10; height: 10 } }
    Item { id: op; objectName: "op"; x: 100; y: 60; width: 20; height: 20; opacity: 0 }
    Item { id: dis; objectName: "dis"; x: 100; y: 90; width: 20; height: 20; enabled: false }
    Item { id: cl; objectName: "cl"; x: 0; y: 100; width: 20; height: 20; clip: true
        Item { id: cc; objectName: "cc"; x: 10; y: 10; width: 40; height: 40 } }
    Item { id: sc; objectName: "sc"; x: 60; y: 100; width: 20; height: 20; scale: 2 }
    Item { id: ro; objectName: "ro"; x: 120; y: 120; width: 40; height: 10; rotation: 90 }
    Item { id: tl; objectName: "tl"; x: 150; y: 0; width: 10; height: 10; scale: 3; transformOrigin: Item.TopLeft }
    Item { id: zero; objectName: "zero"; x: 180; y: 180 }
    Item { id: neg; objectName: "neg"; x: 190; y: 100; width: -10; height: 10 }
    Repeater { model: 2; Item { objectName: "r" + index; x: 180; y: index * 10; width: 5; height: 5 } }
    function n(o) { return o === null ? "null" : o === undefined ? "undefined" : (o.objectName || "?") }
    function at(o, x, y) { var r; try { r = n(o.childAt(x, y)) } catch (e) { r = "" + e } return r }
    Component.onCompleted: {
        var pts = [[10,10],[30,30],[49,49],[50,50],[74,74],[75,75],[105,5],[105,35],[105,65],[105,95],
                   [5,105],[19,119],[20,120],[25,125],[55,95],[50,90],[49,90],[89,129],[90,130],[70,110],
                   [140,125],[139,105],[144,144],[145,145],[135,104],[135,105],[155,5],[179,29],[180,30],[180,180],[185,105],[182,2],[182,12],[-1,-1],[0,0]]
        for (var i = 0; i < pts.length; i++) result += at(root, pts[i][0], pts[i][1]) + " "
        result += "| " + at(cl, 30, 30) + " " + at(cl, 15, 15) + " " + at(hp, 5, 5) + " " + at(zero, 0, 0)
        result += " | " + at(root, "10", "10") + " " + at(root, 10) + " " + at(root) + " " + at(root, NaN, 0) + " " + at(root, 10.5, 10.5) + " " + at(root, Qt.point(10, 10)) + " " + at(root, null, null) + " " + at(root, undefined, 3) + " " + at(root, 10, 10, 99)
        b.z = 10; result += " | " + at(root, 30, 30)
        a.parent = null; a.parent = root; result += " " + at(root, 30, 30) + " " + at(root, 10, 10)
        hid.visible = true; result += " " + at(root, 105, 5)
        result += " " + typeof root.childAt
    }
}
