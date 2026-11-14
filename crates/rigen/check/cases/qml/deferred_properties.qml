// The deferred properties of a control (`deferredNames` of its class: `contentItem`,
// `background`) are not set up with the rest of the object. A value with no `id` inside waits
// for componentComplete of the control, so it is made after the control's children, or for
// the first read of the property; its Component.onCompleted runs in the control's turn either
// way. A value with an `id` inside is set up where it stands, before the children.
// qt: ma,bg,ci ci,ma,bg ma,ci ma,mid kid,ci ma,ci ma,ci ci | after early before delegate loaded nested mid read plain
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
    // Deferred: made in componentComplete of the Button, after its children.
    Button {
        id: plain
        MouseArea { objectName: "ma" }
        contentItem: Item { objectName: "ci"; Component.onCompleted: root.log.push("plain") }
        background: Rectangle { objectName: "bg" }
    }
    // An id inside the value: not deferred, made where the row stands, before the children.
    Button {
        id: withId
        MouseArea { objectName: "ma" }
        contentItem: Item { objectName: "ci"; Item { id: inner } }
        background: Rectangle { objectName: "bg" }
    }
    // Read by a binding before componentComplete: made by that read.
    Button {
        id: read
        MouseArea { objectName: "ma" }
        contentItem: Item { objectName: "ci"; Component.onCompleted: root.log.push("read") }
    }
    property string after: { root.log.push("after"); return "" }
    property string early: { root.log.push("early"); return read.contentItem.objectName }
    property string before: { root.log.push("before"); return "" }
    // A deferred value inside a deferred value.
    Button {
        id: outer
        MouseArea { objectName: "ma" }
        contentItem: Button {
            objectName: "mid"
            Item { objectName: "kid" }
            contentItem: Item { objectName: "ci"; Component.onCompleted: root.log.push("nested") }
            Component.onCompleted: root.log.push("mid")
        }
    }
    Loader {
        id: loader
        sourceComponent: Button {
            MouseArea { objectName: "ma" }
            contentItem: Item { objectName: "ci"; Component.onCompleted: root.log.push("loaded") }
        }
    }
    Repeater {
        id: rep
        model: 1
        Button {
            MouseArea { objectName: "ma" }
            contentItem: Item { objectName: "ci"; Component.onCompleted: root.log.push("delegate") }
        }
    }
    Component.onCompleted: result = [names(plain), names(withId), names(read), names(outer),
        names(outer.contentItem), names(loader.item),
        names(rep.itemAt(0)), early].join(" ") + " | " + log.join(" ")
}
