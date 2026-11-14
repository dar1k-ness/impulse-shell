// qt: a false true e on 0 g 50 0 0 h false true true i 3 0 j 7 8 0
// The C++ property hidden by a QML declaration keeps its own value: what the C++ side does
// with it (enabling children, states, implicit size, a Loader, a Repeater) does not follow
// the declared property.
import QtQuick

Item {
    id: root
    property string result

    Item {
        id: a
        property bool enabled: false
        Item { id: akid }
    }
    Item {
        id: e
        property int v: 0
        property string state: "on"
        states: State { name: "on"; PropertyChanges { target: e; v: 1 } }
    }
    Row {
        id: g
        Item { id: gk; property int implicitWidth: 50; property int implicitHeight: 20 }
    }
    Loader {
        id: h
        property bool active: false
        sourceComponent: Item {}
    }
    Repeater {
        id: i
        property int model: 3
        Item {}
    }
    Repeater {
        id: j
        property int count: 7
        model: 2
        Item {}
    }
    Component.onCompleted: {
        let r = ["a", a.enabled, akid.enabled, "e", e.state, e.v, "g", gk.implicitWidth, g.implicitWidth, gk.width,
                 "h", h.active, h.item !== null, h.status === Loader.Ready, "i", i.model, i.count, "j", j.count, root.children.length]
        e.state = ""
        r.push(e.v)
        result = r.join(" ")
    }
}
