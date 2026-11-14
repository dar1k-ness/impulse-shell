// qt: E0 w(a) lab(a) E18 | load | w(hh) lab(hh) w(hh) E28 | write |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    property real widest: { const ch = host.children; let w = 0; for (let i = 0; i < ch.length; i++) w = Math.max(w, ch[i].w ?? 0); log("E" + w); return w }
    Item {
        id: host
        Repeater {
            id: view
            model: 1
            Item {
                id: tab
                property string name: "a"
                property real w: { root.log("w(" + name + ")"); return lab.implicitWidth + 8 }
                Item { id: lab; implicitWidth: { root.log("lab(" + tab.name + ")"); return tab.name.length * 10 } }
            }
        }
    }
    Component.onCompleted: { mark("load"); view.itemAt(0).name = "hh"; mark("write"); result = seen.join(" ") }
}
