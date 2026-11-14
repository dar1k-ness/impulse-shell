// A typed list property holds its own list, as in Qt 6.12: storing a JS array copies the
// elements, so the array and the property change apart afterwards and are not the same object.
// A `property var` keeps the array itself. Reading the property gives a reference to it:
// `var a = lv; a.push(1)` changes the property. Each step: the signals of lv, pv and li.
// qt: 100,[4,3];100,[9,1];200,[3,4];100,[false,false];100,[true,true,3];010,[4,4,true];001,[4,3];001,[3,4];002,[9,9];200,[2,3,3];100,[4,4,true];
import QtQuick
Item {
    id: root
    property string result: ""
    property list<var> lv: [1, "a"]
    property int nv: 0
    onLvChanged: nv++
    property var pv: [1, "a"]
    property int npv: 0
    onPvChanged: npv++
    property list<int> li: [1, 2]
    property int ni: 0
    onLiChanged: ni++
    function step(f) {
        var n = [nv, npv, ni]
        var r = f()
        result += [nv, npv, ni].map((c, i) => c - n[i]).join("") + "," + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step(() => { var a = [1, 2, 3]; lv = a; a.push(4); return [a.length, lv.length] })
        step(() => { var a = [1, 2, 3]; lv = a; a[0] = 9; return [a[0], lv[0]] })
        step(() => { var a = [1, 2, 3]; lv = a; lv.push(4); return [a.length, lv.length] })
        step(() => { var a = [1, 2, 3]; lv = a; return [lv === a, lv == a] })
        step(() => { var a = [1, , 3]; lv = a; a[1] = 2; return [1 in a, lv[1] === undefined, lv.length] })
        step(() => { var a = [1, 2, 3]; pv = a; a.push(4); return [a.length, pv.length, pv === a] })
        step(() => { var a = [1, 2, 3]; li = a; a.push(4); return [a.length, li.length] })
        step(() => { var a = [1, 2, 3]; li = a; li.push(4); return [a.length, li.length] })
        step(() => { var a = [1, 2, 3]; li = a; a[0] = 9; li = a; return [a[0], li[0]] })
        step(() => { var a = [5, 6]; lv = a; var b = lv; b.push(7); return [a.length, b.length, lv.length] })
        step(() => { var a = lv; a.push(1); return [a.length, lv.length, a === lv] })
    }
}
