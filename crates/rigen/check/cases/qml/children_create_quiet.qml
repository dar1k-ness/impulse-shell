// qt: A[k1,k2,k3] B[ld] | load ran=1,1 | A[k1,k2,k3,?] A[k1,k2,k3,o1] | create ran=2,0 | | create null ran=0,0 | A[k1,k2,k3,o1,o2] | o2->a ran=1,0 | B[ld,?] B[ld,o3] B[ld,o3,?] B[ld,o3,o4] | create two ran=0,4 |
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
        const o1 = kid.createObject(ha, { tag: "o1" }); mark("create")
        const o2 = kid.createObject(null, { tag: "o2" }); mark("create null")
        o2.parent = ha; mark("o2->a")
        const o3 = kid.createObject(hb, { tag: "o3" }); const o4 = kid.createObject(hb, { tag: "o4" }); mark("create two")
        result = seen.join(" ")
    }
}
