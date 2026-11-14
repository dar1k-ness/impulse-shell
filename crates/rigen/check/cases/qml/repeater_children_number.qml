// qt: model count=0 E[-] ch2 t(n0) tw(n0)=28 E[-,28] widest=28 done(n0) added(0,n0) ch3 t(n1) tw(n1)=28 E[28,-,28] done(n1) added(1,n1) ch4 t(n2) tw(n2)=28 E[28,28,-,28] done(n2) added(2,n2) count=3 | load ran=4 | removed(2,n2) gone(n2) removed(1,n1) gone(n1) removed(0,n0) gone(n0) ch3 E[28,28,-] ch2 E[28,-] ch1 E[-] widest=0 ch2 t(n0) tw(n0)=28 E[-,28] widest=28 done(n0) added(0,n0) ch3 t(n1) tw(n1)=28 E[28,-,28] done(n1) added(1,n1) model count=2 | m2 ran=5 | removed(1,n1) gone(n1) removed(0,n0) gone(n0) ch2 E[28,-] ch1 E[-] widest=0 ch2 t(n0) tw(n0)=28 E[-,28] widest=28 done(n0) added(0,n0) model count=1 | m1 ran=3 | removed(0,n0) gone(n0) ch1 E[-] widest=0 model count=0 | m0 ran=1 | ch2 t(n0) tw(n0)=28 E[-,28] widest=28 done(n0) added(0,n0) ch3 t(n1) tw(n1)=28 E[28,-,28] done(n1) added(1,n1) model count=2 | m2b ran=2 |
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
        property real w: { root.log("t(" + name + ")"); return lab.implicitWidth + 8 }
        Item { id: lab; implicitWidth: tab.name.length * 10 }
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
        m = 2; mark("m2")
        m = 1; mark("m1")
        m = 0; mark("m0")
        m = 2; mark("m2b")
        result = seen.join(" ")
    }
}
