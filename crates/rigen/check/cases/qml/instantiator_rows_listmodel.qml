// qt: modelChanged n=0 first=null +a@0 count=3 object=null at1=null added(0,a) count=3 object=a at1=null b.index=1 +b@1 count=3 object=a at1=null added(1,b) count=3 object=a at1=b c.index=2 +c@2 count=3 object=a at1=b added(2,c) count=3 object=a at1=b countChanged count=3 object=a at1=b n=3 | load | b.index=2 c.index=3 m.index=1 +m@1 count=4 object=a at1=null added(1,m) count=4 object=a at1=m countChanged count=4 object=a at1=m n=4 | insert mid | a.index=-1 m.index=0 b.index=1 c.index=2 removed(0,a) count=3 object=m at1=b -a@-1 count=3 object=m at1=b countChanged count=3 object=m at1=b n=3 | remove first | m.index=1 b.index=2 c.index=3 +f@0 count=4 object=null at1=m added(0,f) count=4 object=f at1=m countChanged count=4 object=f at1=m n=4 | insert front | z.index=4 +z@4 count=5 object=f at1=m added(4,z) count=5 object=f at1=m countChanged count=5 object=f at1=m n=5 | append | f.index=2 m.index=0 b.index=1 | move | b.index=-1 f.index=-1 c.index=1 z.index=2 removed(1,b) count=4 object=m at1=f -b@-1 count=4 object=m at1=f removed(1,f) count=3 object=m at1=c -f@-1 count=3 object=m at1=c countChanged count=3 object=m at1=c n=3 | remove two | | set | A.index=-1 c.index=-1 z.index=-1 removed(0,A) count=2 object=c at1=z -A@-1 count=2 object=c at1=z removed(0,c) count=1 object=z at1=null -c@-1 count=1 object=z at1=null removed(0,z) count=0 object=null at1=null -z@-1 count=0 object=null at1=null countChanged count=0 object=null at1=null n=0 | clear | +only@0 count=1 object=null at1=null objectChanged count=1 object=only at1=null first=only added(0,only) count=1 object=only at1=null countChanged count=1 object=only at1=null n=1 | first again | only.index=-1 removed(0,only) count=0 object=null at1=null -only@-1 count=0 object=null at1=null countChanged count=0 object=null at1=null n=0 | last gone |
import QtQuick
import QtQml.Models
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }
    function st() { return "count=" + inst.count + " object=" + (inst.object ? inst.object.name : "null") + " at1=" + (inst.objectAt(1) ? inst.objectAt(1).name : "null") }
    property int n: { root.log("n=" + inst.count); return inst.count }
    property string first: { const o = inst.object; root.log("first=" + (o ? o.name : "null")); return o ? o.name : "" }
    ListModel { id: lm; ListElement { name: "a" } ListElement { name: "b" } ListElement { name: "c" } }
    Instantiator {
        id: inst
        model: lm
        delegate: QtObject {
            required property string name
            required property int index
            onIndexChanged: root.log(name + ".index=" + index)
            Component.onCompleted: root.log("+" + name + "@" + index + " " + root.st())
            Component.onDestruction: root.log("-" + name + "@" + index + " " + root.st())
        }
        onObjectAdded: (i, o) => root.log("added(" + i + "," + o.name + ") " + root.st())
        onObjectRemoved: (i, o) => root.log("removed(" + i + "," + o.name + ") " + root.st())
        onCountChanged: root.log("countChanged " + root.st())
        onObjectChanged: root.log("objectChanged " + root.st())
        onModelChanged: root.log("modelChanged")
    }
    Component.onCompleted: {
        mark("load"); lm.insert(1, { name: "m" }); mark("insert mid")
        lm.remove(0); mark("remove first")
        lm.insert(0, { name: "f" }); mark("insert front")
        lm.append({ name: "z" }); mark("append")
        lm.move(0, 2, 1); mark("move")
        lm.remove(1, 2); mark("remove two")
        lm.set(0, { name: "A" }); mark("set")
        lm.clear(); mark("clear")
        lm.append({ name: "only" }); mark("first again")
        lm.remove(0); mark("last gone")
        result = seen.join(" ")
    }
}
