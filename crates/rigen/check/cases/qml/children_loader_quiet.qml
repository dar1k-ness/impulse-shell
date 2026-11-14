// qt: A[k1,k2,k3] B[] | load ran=1,1 | B[L1] | active ran=0,1 | B[] B[L2] | swap ran=0,2 | B[] | unset ran=0,1 | B[L1] | set ran=0,1 | B[] | inactive ran=0,1 |
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
        ld.active = true; mark("active")
        ld.sourceComponent = c2; mark("swap")
        ld.sourceComponent = undefined; mark("unset")
        ld.sourceComponent = c1; mark("set")
        ld.active = false; mark("inactive")
        result = seen.join(" ")
    }
}
