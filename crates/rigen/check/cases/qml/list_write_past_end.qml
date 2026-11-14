// A typed list has no holes: an element a write past the end or a longer `length` creates
// holds the element type's default value.
// qt: [1,0,0,7];[1,0,0];["a","","z"];[1.5,0,2.5];[true,false,true];3,true;true;a|null|x;a|null;
import QtQuick
Item {
    id: root
    property string result: ""
    property list<int> li: [1]
    property list<int> lj: [1]
    property list<string> ls: ["a"]
    property list<real> lr: [1.5]
    property list<bool> lb: [true]
    property list<url> lu: ["a.txt"]
    property list<var> lv: [1]
    property list<QtObject> lo: [QtObject { objectName: "a" }]
    property list<Item> lt: [Item { objectName: "a" }]
    QtObject { id: x; objectName: "x" }
    Item { id: y; objectName: "y" }
    function names(l) {
        var s = []
        for (var i = 0; i < l.length; i++)
            s.push(l[i] === null ? "null" : l[i] === undefined ? "undefined" : l[i].objectName)
        return s.join("|")
    }
    Component.onCompleted: {
        li[3] = 7; result += JSON.stringify(li) + ";"
        lj.length = 3; result += JSON.stringify(lj) + ";"
        root.ls[2] = "z"; result += JSON.stringify(ls) + ";"
        lr[2] = 2.5; result += JSON.stringify(lr) + ";"
        lb[2] = true; result += JSON.stringify(lb) + ";"
        lu[2] = "b.txt"; result += lu.length + "," + (lu[1] == "") + ";"
        lv[2] = 3; result += (lv[1] === undefined) + ";"
        lo[2] = x; result += names(lo) + ";"
        lt.length = 2; result += names(lt) + ";"
    }
}
