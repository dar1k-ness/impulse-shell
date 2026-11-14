// qt: E[-] E[-,48] E[48,-,28] E[48,28,-,18] | load ran=4 | E[28,18,-] E[18,-] E[-] E[-,68] E[68,-,18] | two ran=5 | E[18,-] E[-] E[-,18] | one ran=3 | E[-] | none ran=1 | E[-,18] E[18,-,28] | two2 ran=2 |
import QtQuick
Item {
    id: root
    width: 400; height: 400
    property string result: ""
    property var seen: []
    property var st: ({ ran: 0, made: 0 })
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " ran=" + st.ran + " |"); st.ran = 0 }

    property var m: ["aaaa", "bb", "c"]
    component Tab: Item {
        id: tab
        property string name: "" + modelData
        width: 10; height: 10
        property real w: { return lab.implicitWidth + 8 }
        Item { id: lab; implicitWidth: tab.name.length * 10 }

    }
    readonly property real widest: {
        let s = [], w = 0
        const ch = host.children
        for (let i = 0; i < ch.length; i++) { s.push(ch[i].w ?? "-"); w = Math.max(w, ch[i].w ?? 0) }
        st.ran++
        log("E[" + s + "]")
        return w
    }

    Item {
        id: host

        Repeater {
            id: view
            model: root.m
            Tab {}

        }
    }
    Component.onCompleted: {
        mark("load")
        m = ["dddddd", "e"]; mark("two")
        m = ["e"]; mark("one")
        m = []; mark("none")
        m = ["f", "gg"]; mark("two2")
        result = seen.join(" ")
    }
}
