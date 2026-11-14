// A `visible` or `enabled` binding is enabled in QQmlObjectCreator::finalize, after the
// children exist: until then the item keeps the constructor's flag, so a child made under it
// hears nothing, a first value equal to the flag changes nothing, and a first value of false
// reaches every item below once (setEffectiveVisibleRecur, setEffectiveEnableRecur).
// qt: i.visible h.visible g.visible h.enabled g.enabled [ n.visible m.visible n.enabled m.enabled ] n.visible m.visible n.enabled m.enabled |
import QtQuick
Item {
    id: root
    property string result: ""
    property bool off: false
    property bool on: true
    function add(x) { result += (result ? " " : "") + x }
    Item {
        id: g
        visible: root.off
        enabled: root.off
        onVisibleChanged: root.add("g.visible")
        onEnabledChanged: root.add("g.enabled")
        Item {
            id: h
            onVisibleChanged: root.add("h.visible")
            onEnabledChanged: root.add("h.enabled")
            Item {
                onVisibleChanged: root.add("i.visible")
            }
        }
    }
    Item {
        id: m
        visible: root.on
        enabled: root.on
        onVisibleChanged: root.add("m.visible")
        onEnabledChanged: root.add("m.enabled")
        Item {
            id: n
            onVisibleChanged: root.add("n.visible")
            onEnabledChanged: root.add("n.enabled")
        }
    }
    Component.onCompleted: { add("["); on = false; add("]"); on = true; add("|") }
}
