// qt: model count=0 E[-] ch2 t(s0) tw(s0)=8 E[-,8] widest=8 done(s0) added(0,s0) ch3 t(s1) tw(s1)=18 E[8,-,18] widest=18 done(s1) added(1,s1) ch4 t(s2) tw(s2)=28 E[8,18,-,28] widest=28 done(s2) added(2,s2) count=3 | load ran=4 | removed(2,s2) gone(s2) removed(1,s1) gone(s1) removed(0,s0) gone(s0) ch3 E[18,28,-] ch2 E[28,-] ch1 E[-] widest=0 ch2 t(s3) tw(s3)=38 E[-,38] widest=38 done(s3) added(0,s3) ch3 t(s4) tw(s4)=48 E[38,-,48] widest=48 done(s4) added(1,s4) model count=2 | swap ran=5 | ch4 t(s5) tw(s5)=58 E[38,48,-,58] widest=58 done(s5) added(2,s5) count=3 | append ran=1 | ch5 t(s6) tw(s6)=68 E[38,48,58,-,68] widest=68 done(s6) added(0,s6) count=4 | insert0 ran=1 | removed(1,s3) gone(s3) ch4 E[68,48,58,-] count=3 | remove1 ran=1 | | move ran=0 | | set0 ran=0 | removed(0,s4) gone(s4) ch3 E[68,58,-] removed(0,s6) gone(s6) ch2 E[58,-] widest=58 removed(0,s5) gone(s5) ch1 E[-] widest=0 count=0 | clear ran=3 | ch2 t(s7) tw(s7)=78 E[-,78] widest=78 done(s7) added(0,s7) count=1 | append2 ran=1 | removed(0,s7) gone(s7) ch1 E[-] widest=0 ch2 t(s8) tw(s8)=88 E[-,88] widest=88 done(s8) added(0,s8) ch3 t(s9) tw(s9)=98 E[88,-,98] widest=98 done(s9) added(1,s9) ch4 t(s10) tw(s10)=108 E[88,98,-,108] widest=108 done(s10) added(2,s10) model count=3 | swapback ran=4 |
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
        property real w: { root.log("t(" + name + ")"); return lab.implicitWidth + 8 }
        Item { id: lab; implicitWidth: tab.serial * 10 }
        onWChanged: root.log("tw(" + name + ")=" + w)
        Component.onCompleted: root.log("done(" + name + ")")
        Component.onDestruction: root.log("gone(" + name + ")")
    }
    readonly property real widest: {
        let s = [], w = 0
        const ch = host.children
        for (let i = 0; i < ch.length; i++) { s.push(ch[i].w ?? "-"); w = Math.max(w, ch[i].w ?? 0) }
        st.ran++
        log("E[" + s + "]")
        return w
    }
    onWidestChanged: log("widest=" + widest)
    Item {
        id: host
        // What the host says while it is created itself is not this case.
        onChildrenChanged: if (root.seen.length) root.log("ch" + children.length)
        Repeater {
            id: view
            model: root.m
            Tab {}
            onModelChanged: root.log("model")
            onCountChanged: root.log("count=" + count)
            onItemAdded: (i, item) => root.log("added(" + i + "," + item.name + ")")
            onItemRemoved: (i, item) => root.log("removed(" + i + "," + item.name + ")")
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
