// qt: model count=0 E[-] ch2 t(aaaa) tw(aaaa)=48 E[-,48] widest=48 done(aaaa) added(0,aaaa) ch3 t(bb) tw(bb)=28 E[48,-,28] done(bb) added(1,bb) ch4 t(c) tw(c)=18 E[48,28,-,18] done(c) added(2,c) count=3 | load ran=4 | removed(2,c) gone(c) removed(1,bb) gone(bb) removed(0,aaaa) gone(aaaa) ch3 E[28,18,-] widest=28 ch2 E[18,-] widest=18 ch1 E[-] widest=0 ch2 t(dddddd) tw(dddddd)=68 E[-,68] widest=68 done(dddddd) added(0,dddddd) ch3 t(e) tw(e)=18 E[68,-,18] done(e) added(1,e) model count=2 | two ran=5 | removed(1,e) gone(e) removed(0,dddddd) gone(dddddd) ch2 E[18,-] widest=18 ch1 E[-] widest=0 ch2 t(e) tw(e)=18 E[-,18] widest=18 done(e) added(0,e) model count=1 | one ran=3 | removed(0,e) gone(e) ch1 E[-] widest=0 model count=0 | none ran=1 | ch2 t(f) tw(f)=18 E[-,18] widest=18 done(f) added(0,f) ch3 t(gg) tw(gg)=28 E[18,-,28] widest=28 done(gg) added(1,gg) model count=2 | two2 ran=2 |
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
        m = ["dddddd", "e"]; mark("two")
        m = ["e"]; mark("one")
        m = []; mark("none")
        m = ["f", "gg"]; mark("two2")
        result = seen.join(" ")
    }
}
