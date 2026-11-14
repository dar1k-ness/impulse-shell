// The other side of the copy a `var` property holds of a sequence, as in Qt 6.12: a write into
// the copy (`laps.push(8)`, `w[0] = 5`, `w.length = 0`) leaves the list it was read from as it
// was, and a local variable read from the `var` property is that same copy. A list of objects
// (`list<Item>`, `list<QtObject>`) is not copied: a `var` property holds a reference to the
// list and sees what is appended to it afterwards; each read of the list property is another
// reference, so assigning it again sends the change signal again; such a list is no JavaScript
// array. Each step: the signals of laps, w, bi, blit, wlit, wlo and of the lists st.laps, st.li,
// st.lit, st.lo, then its value.
// qt: 0000000000,[[1],[1,8]];0000000000,[[1],[0,8]];0100000000,[[1],[5]];0000000000,[[1],[]];0100000000,[[1],[1,2]];0000000000,[[1],[1,2,6],[1,2,6]];0000000000,[[1],[1,4]];0100000000,[[1],[7]];0100000000,[["a"],["a","z"]];0001000010,[1,1];0000100000,1;0000100000,1;0001000010,[2,2,2,true];0001000010,[3,3,3,3];0000010001,[1,1];0000000000,[false,"object","object"];
import QtQuick
Item {
    id: root
    property string result: ""
    property QtObject st: QtObject {
        property list<var> laps: [1]
        property list<int> li: [1]
        property list<string> ls: ["a"]
        property list<QtObject> lo
        property list<Item> lit
    }
    property QtObject o1: QtObject {}
    property Item it1: Item {}
    property Item it2: Item {}
    property var laps: st.laps
    property int nl: 0
    onLapsChanged: nl++
    property var w
    property int nw: 0
    onWChanged: nw++
    property var bi: st.li
    property int nbi: 0
    onBiChanged: nbi++
    property var blit: st.lit
    property int nblit: 0
    onBlitChanged: nblit++
    property var wlit
    property int nwlit: 0
    onWlitChanged: nwlit++
    property var wlo
    property int nwlo: 0
    onWloChanged: nwlo++
    property int nsl: 0
    property int nsli: 0
    property int nslit: 0
    property int nslo: 0
    Connections {
        target: root.st
        function onLapsChanged() { root.nsl++ }
        function onLiChanged() { root.nsli++ }
        function onLitChanged() { root.nslit++ }
        function onLoChanged() { root.nslo++ }
    }
    function counts() { return [nl, nw, nbi, nblit, nwlit, nwlo, nsl, nsli, nslit, nslo] }
    function step(f) {
        var n = counts()
        var r
        try { r = f() } catch (e) { r = "E:" + e }
        result += counts().map((c, i) => c - n[i]).join("") + "," + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step(() => { laps.push(8); return [st.laps, laps] })
        step(() => { laps[0] = 0; return [st.laps, laps] })
        step(() => { w = st.laps; w[0] = 5; return [st.laps, w] })
        step(() => { w.length = 0; return [st.laps, w] })
        step(() => { w = st.laps; w.push(2); w = w; return [st.laps, w] })
        step(() => { var a = w; a.push(6); return [st.laps, w, a] })
        step(() => { bi.push(4); return [st.li, bi] })
        step(() => { w = st.li; w[0] = 7; return [st.li, w] })
        step(() => { w = st.ls; w.push("z"); return [st.ls, w] })
        step(() => { st.lit.push(it1); return [blit.length, st.lit.length] })
        step(() => { wlit = st.lit; return wlit.length })
        step(() => { wlit = st.lit; return wlit.length })
        step(() => { st.lit.push(it2); return [wlit.length, st.lit.length, blit.length, wlit[1] === it2] })
        step(() => { var a = st.lit; a.push(it1); return [a.length, wlit.length, st.lit.length, blit.length] })
        step(() => { wlo = st.lo; st.lo.push(o1); return [wlo.length, st.lo.length] })
        step(() => { return [Array.isArray(st.lit), typeof wlit, typeof st.lit] })
    }
}
