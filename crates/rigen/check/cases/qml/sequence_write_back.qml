// A sequence changed in place is stored back to its property (`li.push(9)`, `li[0] = 7`,
// `lv.length = 1`, by the bare name, `root.` or `this.`): that is no assignment, so in Qt 6.12
// the property keeps its binding and follows `n` afterwards. An assignment (`li = [0]`,
// `root.lv = lv`, `st.li = st.li`) removes the binding. Each step: li, lv, ls, va, st.li, laps,
// blit.length, len, then the signals of li, lv, st.li and blit so far.
// qt: [[1,2,9,8],[1,9,8],["a1","z"],[1,9],[1,9],[1,9],0,4,3,3,1,1];[[7,2,9,8],[1],["y","z"],[1,9],[1,9],[1,9,3],1,4,4,4,1,2];[[5,6],[5],["a5"],[5],[5],[5],1,2,5,5,2,2];[[5,6,4],[5],["a5"],[5],[5],[5],0,3,6,5,2,3];[[0],[5],["a5"],[5],[5],[5],0,1,7,6,2,3];[[0],[5],["a6"],[6],[5],[5],0,1,7,6,2,3];
import QtQuick
Item {
    id: root
    property string result: ""
    property int n: 1
    property list<int> li: [n, n + 1]
    property list<var> lv: [n]
    property list<string> ls: ["a" + n]
    property var va: [n]
    property QtObject st: QtObject {
        property list<int> li: [root.n]
        property list<Item> lit
    }
    property Item it1: Item {}
    property var laps: st.li
    property var blit: st.lit
    property int len: li.length
    property int c: 0
    onLiChanged: c++
    property int cv: 0
    onLvChanged: cv++
    property int cs: 0
    property int nb: 0
    onBlitChanged: nb++
    Connections {
        target: root.st
        function onLiChanged() { root.cs++ }
    }
    function all() { return JSON.stringify([li, lv, ls, va, st.li, laps, blit.length, len, c, cv, cs, nb]) + ";" }
    Component.onCompleted: {
        li.push(9); lv.push(9); ls.push("z"); va.push(9); st.li.push(9); root.li.push(8); this.lv.push(8)
        result += all()
        li[0] = 7; lv.length = 1; root.ls[0] = "y"; laps.push(3); st.lit.push(it1)
        result += all()
        n = 5
        result += all()
        li.push(4); st.lit = []
        result += all()
        li = [0]; root.lv = lv; st.li = st.li
        result += all()
        n = 6
        result += all()
    }
}
