// A deferred property set by two layers of a type keeps the outer value only; a script value
// is deferred as an object is; a write before componentComplete drops the deferred value; a
// class executes its own deferred properties before its base's (`indicator`, `handle`), and
// a Loader inside a deferred value loads when that value completes.
// qt: ma,outer ma,outer ma ma ma,ind,bg,ci ma,ci ma,hd,bg spare,spare2 outer | h-hd h-bg g-loaded g-ci f-ind f-bg f-ci a-outer
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string result: ""
    readonly property var log: []
    function names(o) {
        var s = []
        for (var i = 0; i < o.children.length; i++)
            if (o.children[i].objectName)
                s.push(o.children[i].objectName)
        return s.join(",")
    }
    Item { id: spare; objectName: "spare" }
    Item { id: spare2; objectName: "spare2" }
    // Both layers deferred: only the outer value is made.
    Plain {
        id: a; log: root.log; tag: "a"
        contentItem: Item { objectName: "outer"; Component.onCompleted: root.log.push("a-outer") }
    }
    // The outer value has an id: set at once, the deferred inner one is dropped.
    Plain {
        id: b; log: root.log; tag: "b"
        contentItem: Item { objectName: "outer"; Item { id: y } Component.onCompleted: root.log.push("b-outer") }
    }
    // A script value is deferred as well.
    Button {
        id: d
        MouseArea { objectName: "ma" }
        contentItem: spare
    }
    // A write before the button completes drops the deferred value.
    Button {
        id: e
        MouseArea { objectName: "ma" }
        contentItem: Item { objectName: "ci"; Component.onCompleted: root.log.push("e-ci") }
        property int w: { e.contentItem = spare2; return 1 }
    }
    // Each class executes its own: indicator, background, contentItem.
    CheckBox {
        id: f
        MouseArea { objectName: "ma" }
        contentItem: Item { objectName: "ci"; Component.onCompleted: root.log.push("f-ci") }
        background: Item { objectName: "bg"; Component.onCompleted: root.log.push("f-bg") }
        indicator: Item { objectName: "ind"; Component.onCompleted: root.log.push("f-ind") }
    }
    // A Loader inside a deferred value.
    Button {
        id: g
        MouseArea { objectName: "ma" }
        contentItem: Item {
            objectName: "ci"
            Loader { objectName: "ld"; sourceComponent: Item { Component.onCompleted: root.log.push("g-loaded") } }
            Component.onCompleted: root.log.push("g-ci")
        }
        Component.onCompleted: root.log.push("g")
    }
    Slider {
        id: h
        MouseArea { objectName: "ma" }
        handle: Item { objectName: "hd"; Component.onCompleted: root.log.push("h-hd") }
        background: Item { objectName: "bg"; Component.onCompleted: root.log.push("h-bg") }
    }
    Component.onCompleted: result = [names(a), names(b), names(d), names(e), names(f),
        names(g), names(h), names(spare.parent ?? root), a.contentItem.objectName].join(" ")
        + " | " + log.join(" ")
}
