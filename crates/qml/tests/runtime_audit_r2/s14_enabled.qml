import QtQuick
Item {
    id: r
    Item { id: a
        Item { id: b; onEnabledChanged: console.log("b.enabledChanged", enabled)
            property bool e: enabled
            onEChanged: console.log("binding e", e)
            Item { id: c; enabled: false; onEnabledChanged: console.log("c.enabledChanged", enabled) }
            TextInput { id: ti; focus: true }
        }
    }
    Component.onCompleted: {
        a.enabled = false; console.log("b", b.enabled, "c", c.enabled)
        b.enabled = true; console.log("b after own true", b.enabled)
        a.enabled = true; console.log("b", b.enabled, "c", c.enabled)
        c.enabled = true; console.log("c", c.enabled)
    }
}
