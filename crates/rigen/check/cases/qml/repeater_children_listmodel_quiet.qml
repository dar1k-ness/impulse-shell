// qt: E[-] E[-,8] E[8,-,18] E[8,18,-,28] | load ran=4 | E[18,28,-] E[28,-] E[-] E[-,38] E[38,-,48] | swap ran=5 | E[38,48,-,58] | append ran=1 | E[38,48,58,-,68] | insert0 ran=1 | E[68,48,58,-] | remove1 ran=1 | | move ran=0 | | set0 ran=0 | E[68,58,-] E[58,-] E[-] | clear ran=3 | E[-,78] | append2 ran=1 | E[-] E[-,88] E[88,-,98] E[88,98,-,108] | swapback ran=4 |
import QtQuick
Item {
    id: root
    width: 400; height: 400
    property string result: ""
    property var seen: []
    property var st: ({ ran: 0, made: 0 })
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " ran=" + st.ran + " |"); st.ran = 0 }
    ListModel { id: la; ListElement { role: "aaaa" } ListElement { role: "bb" } ListElement { role: "c" } }
    ListModel { id: lb; ListElement { role: "dddddd" } ListElement { role: "e" } }
    property var m: la
    component Tab: Item {
        id: tab
        readonly property int serial: root.st.made++
        property string name: "s" + serial
        width: 10; height: 10
        property real w: { return lab.implicitWidth + 8 }
        Item { id: lab; implicitWidth: tab.serial * 10 }

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
        m = lb; mark("swap")
        lb.append({ role: "ff" }); mark("append")
        lb.insert(0, { role: "ggg" }); mark("insert0")
        lb.remove(1); mark("remove1")
        lb.move(0, 1, 1); mark("move")
        lb.set(0, { role: "hhhhh" }); mark("set0")
        lb.clear(); mark("clear")
        lb.append({ role: "i" }); mark("append2")
        m = la; mark("swapback")
        result = seen.join(" ")
    }
}
