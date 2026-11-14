import QtQuick
import QtQuick.Controls
Item {
    id: r
    function n(o) { return o ? o.objectName : o }
    onChildrenChanged: console.log("r.children", children.length)
    Item { id: a; objectName: "a"; enabled: false
        onChildrenChanged: console.log("a.children", children.length)
        onEnabledChanged: console.log("a.enabled", enabled)
        Item { id: a1; objectName: "a1"
            onParentChanged: console.log("a1.parent", r.n(parent))
            onEnabledChanged: console.log("a1.enabled", enabled)
            onVisibleChanged: console.log("a1.visible", visible)
            Item { id: a11; objectName: "a11"
                onParentChanged: console.log("a11.parent", r.n(parent))
                onEnabledChanged: console.log("a11.enabled", enabled) }
        }
    }
    Item { id: b; objectName: "b"
        Item { id: b1; objectName: "b1"
            onParentChanged: console.log("b1.parent", r.n(parent))
            onEnabledChanged: console.log("b1.enabled", enabled)
            onVisibleChanged: console.log("b1.visible", visible)
            Item { id: b11; objectName: "b11"; enabled: false
                onEnabledChanged: console.log("b11.enabled", enabled)
                onVisibleChanged: console.log("b11.visible", visible) }
            Item { id: b12; objectName: "b12"; visible: false
                onEnabledChanged: console.log("b12.enabled", enabled)
                onVisibleChanged: console.log("b12.visible", visible) }
        }
        enabled: false
        visible: false
        onVisibleChanged: console.log("b.visible", visible)
    }
    Item { id: c; objectName: "c"; visible: false
        Item { id: c1; objectName: "c1"
            onParentChanged: console.log("c1.parent", r.n(parent))
            onVisibleChanged: console.log("c1.visible", visible) }
        QtObject { id: cq; objectName: "cq" }
    }
    Item { id: d; objectName: "d"; width: 5; height: 6; x: 1; opacity: 0.5; z: 2; clip: true
        onWidthChanged: console.log("d.width", width)
        onHeightChanged: console.log("d.height", height)
        onXChanged: console.log("d.x", x)
        onOpacityChanged: console.log("d.opacity", opacity)
        onZChanged: console.log("d.z", z)
        onClipChanged: console.log("d.clip", clip)
        onObjectNameChanged: console.log("d.objectName", objectName)
    }
    Item { id: e; objectName: "e"
        property alias iw: e1.width
        property alias en: e1.objectName
        property int own: 3
        onIwChanged: console.log("e.iw", iw)
        onEnChanged: console.log("e.en", en)
        onOwnChanged: console.log("e.own", own)
        Item { id: e1; onWidthChanged: console.log("e1.width", width); onObjectNameChanged: console.log("e1.objectName", objectName) }
    }
    Item { id: f; objectName: "f"
        property Item pv: Item { objectName: "pv"; onParentChanged: console.log("pv.parent", r.n(parent)) }
        property QtObject q: QtObject { objectName: "pq"; onObjectNameChanged: console.log("pq.name") }
        onPvChanged: console.log("f.pv", r.n(pv))
    }
    Text { id: t1; text: "t1"; onTextChanged: console.log("Text.text", text) }
    TextInput { id: t2; text: "t2"; onTextChanged: console.log("TextInput.text", text); onDisplayTextChanged: console.log("TextInput.display", displayText); onCursorPositionChanged: console.log("TextInput.cursor", cursorPosition); onLengthChanged: console.log("TextInput.length", length) }
    TextEdit { id: t3; text: "t3"; onTextChanged: console.log("TextEdit.text", text); onCursorPositionChanged: console.log("TextEdit.cursor", cursorPosition); onLengthChanged: console.log("TextEdit.length", length) }
    TextField { id: t4; text: "t4"; onTextChanged: console.log("TextField.text", text) }
    TextArea { id: t5; text: "t5"; onTextChanged: console.log("TextArea.text", text) }
    Label { id: t6; text: "t6"; onTextChanged: console.log("Label.text", text) }
    Rectangle { id: g; color: "red"; radius: 3; border.width: 2; border.color: "blue"
        onColorChanged: console.log("g.color", color)
        onRadiusChanged: console.log("g.radius", radius) }
    Item { id: k; state: "s"
        property int p: 1
        onPChanged: console.log("k.p", p)
        states: State { name: "s"; PropertyChanges { target: k; p: 2 } } }
    Component.onCompleted: console.log("done", r.n(f.pv.parent), a1.enabled, b1.enabled, b1.visible, c1.visible)
}
