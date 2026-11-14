import QtQuick
Item {
    id: r
    Item { id: a; objectName: "a"; onChildrenChanged: console.log("  a.childrenChanged", children.length)
        Item { id: a1; objectName: "a1"; onParentChanged: console.log("  a1.parentChanged", parent ? parent.objectName : parent); onVisibleChanged: console.log("  a1.visible", visible) }
        Item { id: a2; objectName: "a2"; onParentChanged: console.log("  a2.parentChanged", parent ? parent.objectName : parent) }
        QtObject { id: aq; objectName: "aq" }
        Timer { id: at; objectName: "at" }
    }
    Item { id: b; objectName: "b"; onChildrenChanged: console.log("  b.childrenChanged", children.length) }
    Item { id: c; objectName: "c"; property Item pv: Item { objectName: "pv" }
        onChildrenChanged: console.log("  c.childrenChanged", children.length)
        Item { objectName: "c1" } QtObject { id: cq; objectName: "cq" } }
    Item { id: d; objectName: "d"; children: [d2]; Item { id: d1; objectName: "d1" } }
    Item { id: d2; objectName: "d2" }
    Item { id: e; objectName: "e"; property var list: [a1]; children: list; onChildrenChanged: console.log("  e.childrenChanged", children.length) }
    QtObject { id: q; objectName: "q" }
    function names(l) { var s = []; for (var i = 0; i < l.length; i++) s.push(l[i].objectName); return s.join(",") }
    Component.onCompleted: {
        console.log("init a", names(a.children), "|", names(a.resources), "|", names(a.data))
        console.log("init c", names(c.children), "|", names(c.resources), "|", names(c.data))
        console.log("init d", names(d.children), d1.parent, d2.parent.objectName)
        console.log("init e", names(e.children), a1.parent.objectName)
        b.children = [a2]; console.log("b=[a2]", names(a.children), names(b.children))
        b.children = [a2]; console.log("b=[a2] again", names(b.children))
        b.children = [a1, a2]; console.log("b=[a1,a2]", names(b.children))
        b.children = [a2, a1]; console.log("b=[a2,a1]", names(b.children))
        b.children = a1; console.log("b=a1", names(b.children), a2.parent)
        try { b.children = [q]; console.log("b=[q]", names(b.children), a1.parent) } catch (e) { console.log("b=[q] err", e.message) }
        try { b.children = [a2, q, null, a1]; console.log("b=[a2,q,null,a1]", names(b.children)) } catch (e) { console.log("err", e.message) }
        try { b.children = null; console.log("b=null", names(b.children)) } catch (e) { console.log("err", e.message) }
        try { b.children = undefined; console.log("b=undefined", names(b.children)) } catch (e) { console.log("err", e.message) }
        e.list = [a2, a1]; console.log("e.list", names(e.children))
        c.data = [a1, q]; console.log("c.data=[a1,q]", names(c.children), "|", names(c.resources), "|", names(c.data), "r:", names(r.resources))
        c.data = []; console.log("c.data=[]", names(c.children), "|", names(c.resources), "|", names(c.data), a1.parent)
        c.resources = [q, aq]; console.log("c.resources=", names(c.children), "|", names(c.resources), "|", names(c.data), "a:", names(a.resources))
        c.data = a2; console.log("c.data=a2", names(c.children), "|", names(c.resources), "|", names(c.data))
        a.data = [at]; console.log("a.data=[at]", names(a.children), "|", names(a.resources), "|", names(a.data))
        b.children.push(a1); console.log("push", names(b.children))
        b.data.push(q); console.log("data.push", names(b.resources), names(b.data))
        b.children.length = 0; console.log("length=0", names(b.children))
    }
}
