// qt: E[-] E[-,28] E[28,-,28] E[28,28,-,28] | load ran=4 | E[28,28,-] E[28,-] E[-] E[-,28] E[28,-,28] | m2 ran=5 | E[28,-] E[-] E[-,28] | m1 ran=3 | E[-] | m0 ran=1 | E[-,28] E[28,-,28] | m2b ran=2 |
import QtQuick
Item {
    id: root
    width: 400; height: 400
    property string result: ""
    property var seen: []
    property var st: ({ ran: 0, made: 0 })
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " ran=" + st.ran + " |"); st.ran = 0 }

    property var m: 3
    component Tab: Item {
        id: tab
        property string name: "n" + modelData
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
        m = 2; mark("m2")
        m = 1; mark("m1")
        m = 0; mark("m0")
        m = 2; mark("m2b")
        result = seen.join(" ")
    }
}
