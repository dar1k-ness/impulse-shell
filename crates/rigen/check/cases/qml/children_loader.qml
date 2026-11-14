// qt: A[k1,k2,k3] na=3 B[] | load ran=1,1 | B[L1] nb=1 | active ran=0,1 | gone(L1) B[] nb=0 B[L2] nb=1 | swap ran=0,2 | gone(L2) B[] nb=0 | unset ran=0,1 | B[L1] nb=1 | set ran=0,1 | gone(L1) B[] nb=0 | inactive ran=0,1 |
import QtQuick
Item {
    id: root
    width: 400; height: 400
    property string result: ""
    property var seen: []
    property var st: ({ a: 0, b: 0 })
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " ran=" + st.a + "," + st.b + " |"); st.a = 0; st.b = 0 }
    function tags(it) { let s = []; const ch = it.children; for (let i = 0; i < ch.length; i++) s.push(ch[i].tag ?? "-"); return s }
    readonly property int na: { const s = tags(ha); st.a++; log("A[" + s + "]"); return s.length }
    readonly property int nb: { const s = tags(ld); st.b++; log("B[" + s + "]"); return s.length }
    component Kid: Item { property string tag: "?"; Component.onDestruction: root.log("gone(" + tag + ")") }
    Component { id: kid; Kid {} }
    Component { id: c1; Kid { tag: "L1" } }
    Component { id: c2; Kid { tag: "L2" } }
    Item { id: ha; onChildrenChanged: if (root.seen.length) root.log("chA" + children.length)
        Kid { id: k1; tag: "k1" }
        Kid { id: k2; tag: "k2" }
        Kid { id: k3; tag: "k3" }
    }
    Item { id: hb; onChildrenChanged: if (root.seen.length) root.log("chB" + children.length)
        Loader { id: ld; property string tag: "ld"; active: false; sourceComponent: c1 }
    }
    onNaChanged: log("na=" + na)
    onNbChanged: log("nb=" + nb)
    Component.onCompleted: {
        mark("load")
        ld.active = true; mark("active")
        ld.sourceComponent = c2; mark("swap")
        ld.sourceComponent = undefined; mark("unset")
        ld.sourceComponent = c1; mark("set")
        ld.active = false; mark("inactive")
        result = seen.join(" ")
    }
}
