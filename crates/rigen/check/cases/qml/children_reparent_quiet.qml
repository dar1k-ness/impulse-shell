// qt: A[k1,k2,k3] B[ld] | load ran=1,1 | A[k2,k3] B[ld,k1] | k1->b ran=1,1 | A[k3] B[ld,k1,k2] A[] B[ld,k1,k2,k3] | k2,k3->b ran=2,2 | B[ld,k2,k3] A[k1] | k1->a ran=1,1 | B[ld,k3] | k2->null ran=0,1 | A[k1,k2] | k2->a ran=1,0 | | same ran=0,0 |
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
    component Kid: Item { property string tag: "?";  }
    Component { id: kid; Kid {} }
    Component { id: c1; Kid { tag: "L1" } }
    Component { id: c2; Kid { tag: "L2" } }
    Item { id: ha; 
        Kid { id: k1; tag: "k1" }
        Kid { id: k2; tag: "k2" }
        Kid { id: k3; tag: "k3" }
    }
    Item { id: hb; 
        Loader { id: ld; property string tag: "ld"; active: false; sourceComponent: c1 }
    }
    
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
