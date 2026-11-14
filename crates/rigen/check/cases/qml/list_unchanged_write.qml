// A typed list property holds a value: a write of the elements it already holds (itself, an
// equal array, its own length, an element's own value) sends no change signal. A write of
// anything else sends one.
// qt: a00000;b00000;c00000;d00000;e00000;f00000;g00000;h10000;i10000;j10000;k10000;l00000;m01000;n00000;o00000;p00100;q00100;r00000;s00001;t00001;
import QtQuick
Item {
    id: root
    property string result: ""
    property list<int> li: [1, 2, 3]
    property int ni: 0
    onLiChanged: ni++
    property list<string> ls: ["a", "b"]
    property int ns: 0
    onLsChanged: ns++
    property list<QtObject> lo: [QtObject { objectName: "a" }]
    property int no: 0
    onLoChanged: no++
    property list<Item> lt: [Item { objectName: "a" }]
    property int nt: 0
    onLtChanged: nt++
    property list<var> lv: [1, "a"]
    property int nv: 0
    onLvChanged: nv++
    QtObject { id: x; objectName: "x" }
    function step(k, f) {
        var n = [ni, ns, no, nt, nv]
        f()
        var now = [ni, ns, no, nt, nv]
        result += k + now.map((c, i) => c - n[i]).join("") + ";"
    }
    Component.onCompleted: {
        step("a", () => { li = li })
        step("b", () => { root.li = root.li })
        step("c", () => { li.length = 3 })
        step("d", () => { root.li.length = 3 })
        step("e", () => { li = [1, 2, 3] })
        step("f", () => { li[0] = 1 })
        step("g", () => { li.sort() })
        step("h", () => { li[0] = 9 })
        step("i", () => { li = [9, 2] })
        step("j", () => { li.push(4) })
        step("k", () => { li.length = 0 })
        step("l", () => { ls = ls; ls.length = 2; ls = ["a", "b"]; ls[1] = "b" })
        step("m", () => { ls[1] = "c" })
        step("n", () => { lo = lo; lt = lt })
        step("o", () => { lo.length = 1; root.lt.length = 1 })
        step("p", () => { lo.push(x) })
        step("q", () => { lo[0] = x })
        step("r", () => { lv.length = 2; root.lv.length = 2 })
        step("s", () => { lv = lv })
        step("t", () => { lv[0] = 1 })
    }
}
