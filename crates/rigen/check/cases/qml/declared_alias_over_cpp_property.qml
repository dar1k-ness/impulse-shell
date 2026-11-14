// qt: 4 4 4 9 9 9 true true false false true in false out out g9efalse
// An alias declared under the name of a C++ property hides it too: the name reads and writes
// the alias target, and the C++ property keeps its own value.
import QtQuick

Item {
    id: root
    property string result
    property string log

    Grid {
        id: g
        property alias spacing: g.rowSpacing
        rowSpacing: 4
        columnSpacing: rowSpacing
        onSpacingChanged: root.log += "g" + spacing
    }
    Item {
        id: e
        property bool on: true
        property alias enabled: e.on
        onEnabledChanged: root.log += "e" + enabled
        Item { id: kid }
    }
    Text {
        id: t
        property alias text: inner.text
        Text { id: inner; text: "in" }
    }
    Component.onCompleted: {
        let r = [g.spacing, g.rowSpacing, g.columnSpacing]
        g.spacing = 9
        r.push(g.spacing, g.rowSpacing, g.columnSpacing)
        r.push(e.enabled, kid.enabled)
        e.enabled = false
        r.push(e.enabled, e.on, kid.enabled)
        r.push(t.text, t.implicitWidth > 0)
        t.text = "out"
        r.push(t.text, inner.text)
        result = r.join(" ") + " " + log
    }
}
