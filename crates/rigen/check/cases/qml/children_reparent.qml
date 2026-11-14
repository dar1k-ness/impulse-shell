// qt: A[k1,k2,k3] na=3 B[ld] nb=1 | load ran=1,1 | chA2 A[k2,k3] na=2 chB2 B[ld,k1] nb=2 | k1->b ran=1,1 | chA1 A[k3] na=1 chB3 B[ld,k1,k2] nb=3 chA0 A[] na=0 chB4 B[ld,k1,k2,k3] nb=4 | k2,k3->b ran=2,2 | chB3 B[ld,k2,k3] nb=3 chA1 A[k1] na=1 | k1->a ran=1,1 | chB2 B[ld,k3] nb=2 | k2->null ran=0,1 | chA2 A[k1,k2] na=2 | k2->a ran=1,0 | | same ran=0,0 |
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
    readonly property int nb: { const s = tags(hb); st.b++; log("B[" + s + "]"); return s.length }
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
        k1.parent = hb; mark("k1->b")
        k2.parent = hb; k3.parent = hb; mark("k2,k3->b")
        k1.parent = ha; mark("k1->a")
        k2.parent = null; mark("k2->null")
        k2.parent = ha; mark("k2->a")
        k3.parent = hb; mark("same")
        result = seen.join(" ")
    }
}
