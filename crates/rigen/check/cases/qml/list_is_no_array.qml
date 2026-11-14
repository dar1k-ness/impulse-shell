// The value of a `list<T>` property is no Array in Qt 6.12: `Array.isArray` is false and
// `Object.prototype.toString` names a sequence (`list<QtObject>`: a QML list), while
// `instanceof Array` and the Array methods hold, and what those methods return is an Array.
// Each step: the signals of lv and li it sent, then its value.
// Not as Qt, and not here: a reference read before the property is assigned another array
// follows the property in Qt (`var a = lv; lv = [1, 2]`: `a` is `[1, 2]`); an object stored
// in a `list<var>` is converted (a copy, another one on each read of the element); the list
// has no own `length` and index properties.
// qt: a00,[false,false,false,false,false,false,true];b00,["[object V4Sequence]","[object V4Sequence]","[object V4Sequence]","[object V4Sequence]","[object QML List]","[object QML List]","[object Array]"];c00,[true,true,true,"object","object"];h00,[true,true,true];i10,[true,null,true,"s",1.5,true];j00,[2,true];k10,[[2,4,6],[1,2,3,4],true,true,true,"[1,2,3]",3,3];l00,[[1,2,3],[0,1,2,3,1,2,3],true,"1,2,3","1,2,3","1,2,3"];
import QtQuick
Item {
    id: root
    property string result: ""
    property list<var> lv: [1, 2, 3]
    property int nlv: 0
    onLvChanged: nlv++
    property list<int> li: [1, 2, 3]
    property int nli: 0
    onLiChanged: nli++
    property list<string> ls: ["a"]
    property list<real> lr: [1.5]
    property list<QtObject> lo: [QtObject {}]
    property list<Item> lit
    property var pv: [1, 2]
    function step(k, f) {
        var n = [nlv, nli]
        var r
        try { r = f() } catch (e) { r = e.name + ":" + e.message }
        var now = [nlv, nli]
        result += k + now.map((c, i) => c - n[i]).join("") + "," + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step("a", () => [Array.isArray(lv), Array.isArray(li), Array.isArray(ls), Array.isArray(lr), Array.isArray(lo), Array.isArray(lit), Array.isArray(pv)])
        step("b", () => [lv, li, ls, lr, lo, lit, pv].map(x => Object.prototype.toString.call(x)))
        step("c", () => [lv instanceof Array, li instanceof Array, lo instanceof Array, typeof lv, typeof lo])
        step("h", () => { var a = lv; var b = lv; return [a === b, lv === lv, a == b] })
        step("i", () => { lv = [root, null, undefined, "s", 1.5, true]; return [lv[0] === root, lv[1], lv[2] === undefined, lv[3], lv[4], lv[5]] })
        step("j", () => { var o = { x: 1 }; pv = [o]; o.x = 2; return [pv[0].x, pv[0] === o] })
        step("k", () => { lv = [1, 2, 3]; return [lv.map(x => x * 2), lv.concat([4]), Array.isArray(lv.slice()), Array.isArray(lv.map(x => x)), Array.isArray(lv.concat([])), JSON.stringify(lv), Array.from(lv).length, [...lv].length] })
        step("l", () => { return [[].concat(lv), [0].concat(lv, li), Array.isArray(Array.from(li)), String(lv), lv.toString(), lv + ""] })
    }
}
