import QtQuick
Item {
    id: r
    Component { id: c
        Item { objectName: "top"
            Component.onDestruction: console.log("destr top")
            Item { objectName: "k1"; Component.onDestruction: console.log("destr k1")
                Item { Component.onDestruction: console.log("destr k1a") } }
            Item { objectName: "k2"; Component.onDestruction: console.log("destr k2") }
            property QtObject q: QtObject { Component.onDestruction: console.log("destr q") }
            Timer { Component.onDestruction: console.log("destr timer") }
            Repeater { model: 2; Item { Component.onDestruction: console.log("destr rep", index) } }
            Loader { sourceComponent: Item { Component.onDestruction: console.log("destr loaded") } }
        }
    }
    signal sig(int x)
    function slot(x) { console.log("slot", x) }
    Component.onCompleted: {
        var o = c.createObject(r); o.destroy()
        var f = function(x) { console.log("f", x, this === r) }
        sig.connect(f); sig.connect(f); sig.connect(slot); sig.connect(r, f); sig(1)
        sig.disconnect(f); sig(2)
        sig.disconnect(f); sig.disconnect(slot); sig(3)
        try { sig.disconnect(f); console.log("disconnect unknown ok") } catch (e) { console.log("disconnect err:", e.message) }
        try { sig.connect(5) } catch (e) { console.log("connect err:", e.message) }
        try { sig.disconnect() } catch (e) { console.log("disconnect() err:", e.message) }
    }
}
