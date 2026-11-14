// A `var` property holds its own copy of a sequence (a `list<var>`, `list<int>`, `list<string>`
// property), as in Qt 6.12: each write of the list is another value and sends the change signal
// (`w = st.laps` twice, a binding that runs again), `w = w` sends none, and the copy and the list
// change apart afterwards. So `property var laps: st.laps` follows `st.laps.push(5)`, and what is
// bound to `laps` with it. A list of objects stays a reference to the one list, and notifies too.
// A local variable and an object's field keep a reference to the list. Each step: the signals of
// laps, w, vx, bi, bs, ba, blo and wlo, then its value.
// qt: 10100000,[1,[5]];10100000,[2,[7,5]];10100000,[2,[7,9]];10100000,[0,[]];10100000,[1,[9],false,true,true];01000000,[9];01000000,[9];00000000,[9];12100000,[[9,1],[9],[9,1]];00000000,[[9,1],[9,3],[9,1]];00100000,[9,1];00010000,[1,2];00000000,[1,2];00001000,["a","b"];00000000,[1,2];00000010,1;00000012,[2,2];10100000,[[9,1,4],[9,3],[9,1,4]];
import QtQuick
Item {
    id: root
    property string result: ""
    property QtObject st: QtObject {
        property list<var> laps: []
        property list<int> li: [1]
        property list<string> ls: ["a"]
        property list<QtObject> lo
    }
    property QtObject o1: QtObject {}
    property var laps: st.laps
    property int nl: 0
    onLapsChanged: nl++
    property int len: laps.length
    property var rev: laps.map((v, i, arr) => arr[arr.length - 1 - i])
    property var w
    property int nw: 0
    onWChanged: nw++
    property bool flag: false
    property var vx: flag ? st.laps : st.laps
    property int nx: 0
    onVxChanged: nx++
    property variant bi: st.li
    property int nbi: 0
    onBiChanged: nbi++
    property var bs: st.ls
    property int nbs: 0
    onBsChanged: nbs++
    property var arr: [1]
    property var ba: arr
    property int nba: 0
    onBaChanged: nba++
    property var blo: st.lo
    property int nblo: 0
    onBloChanged: nblo++
    property var wlo
    property int nwlo: 0
    onWloChanged: nwlo++
    function counts() { return [nl, nw, nx, nbi, nbs, nba, nblo, nwlo] }
    function step(f) {
        var n = counts()
        var r = f()
        result += counts().map((c, i) => c - n[i]).join("") + "," + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step(() => { st.laps.push(5); return [len, rev] })
        step(() => { st.laps.push(7); return [len, rev] })
        step(() => { st.laps[0] = 9; return [len, rev] })
        step(() => { st.laps = []; return [len, rev] })
        step(() => { st.laps.push(9); return [len, rev, laps === st.laps, st.laps === st.laps, laps === laps] })
        step(() => { w = st.laps; return w })
        step(() => { w = st.laps; return w })
        step(() => { w = w; return w })
        step(() => { var a = st.laps; w = a; w = a; st.laps.push(1); return [a, w, laps] })
        step(() => { w.push(3); return [st.laps, w, laps] })
        step(() => { flag = true; return vx })
        step(() => { st.li.push(2); return bi })
        step(() => { st.li[0] = 1; return bi })
        step(() => { st.ls.push("b"); return bs })
        step(() => { arr.push(2); return ba })
        step(() => { st.lo.push(o1); return blo.length })
        step(() => { wlo = st.lo; wlo = st.lo; st.lo.push(o1); return [wlo.length, blo.length] })
        step(() => { var o = { l: st.laps }; st.laps.push(4); return [o.l, w, laps] })
    }
}
