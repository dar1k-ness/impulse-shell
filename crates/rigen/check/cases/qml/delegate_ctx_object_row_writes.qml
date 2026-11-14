// A row that is a QObject, written from its delegate (Qt 6.12). `model.<name> = v` goes by the
// object's metacall (QQmlDMObjectDataMetaObject): the property keeps its binding and follows
// `n` afterwards. The bare name is an assignment to the object's property and removes the
// binding. A sequence changed in place (`li.push`, `model.li.push`, `modelData.li.push`) is
// stored back and is no assignment. Each row: li, k, the signals of li.
// qt: byName [1,2,9] [1,2,9]/10/2;byModel [1,2,8] [1,2,8]/10/2;byData [1,2,7] [1,2,7]/10/2;plain 3 [1,2]/3/1;assign [0] [0]/3/2;bare 4 [4] [4]/4/2;after [5,6]/50/3 [5,6]/50/3 [5,6]/50/3 [5,6]/50/3 [4]/4/2
import QtQuick
Item {
    id: root
    property string result: ""
    property int n: 1
    property var seen: []
    component Row: QtObject {
        property list<int> li: [root.n, root.n + 1]
        property int k: root.n * 10
        property int c: 0
        onLiChanged: c++
    }
    property Row a: Row {}
    property Row b: Row {}
    property Row d: Row {}
    property Row e: Row {}
    property Row f: Row {}
    property list<QtObject> rows: [a, b, d, e, f]
    Repeater { id: rep; model: root.rows; delegate: Item {
        function byName() { li.push(9); return JSON.stringify(li) }
        function byModel() { model.li.push(8); return JSON.stringify(model.li) }
        function byData() { modelData.li.push(7); return JSON.stringify(modelData.li) }
        function plain() { model.k = 3; return k }
        function bare() { k = 4; return k }
        function bareList() { li = [4]; return JSON.stringify(li) }
        function assign() { model.li = [0]; return JSON.stringify(li) }
    } }
    function st(o) { return JSON.stringify(o.li) + "/" + o.k + "/" + o.c }
    Component.onCompleted: {
        seen.push("byName " + rep.itemAt(0).byName() + " " + st(a))
        seen.push("byModel " + rep.itemAt(1).byModel() + " " + st(b))
        seen.push("byData " + rep.itemAt(2).byData() + " " + st(d))
        seen.push("plain " + rep.itemAt(3).plain() + " " + st(e))
        seen.push("assign " + rep.itemAt(3).assign() + " " + st(e))
        seen.push("bare " + rep.itemAt(4).bare() + " " + rep.itemAt(4).bareList() + " " + st(f))
        n = 5
        seen.push("after " + st(a) + " " + st(b) + " " + st(d) + " " + st(e) + " " + st(f))
        result = seen.join(";")
    }
}
