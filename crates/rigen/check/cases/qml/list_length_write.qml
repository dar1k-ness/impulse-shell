// A write to `length` of a list property reached by its bare name is written back to the
// property like any other change of the list: the change signal is sent and a binding that
// reads the list runs again.
// qt: a1,5,5;b1,2,2;c1,4,4;d1,1,1;e1,1,1;f1,2,2;g1,1,1;
import QtQuick
Item {
    id: root
    property string result: ""
    property list<int> la: [1, 2, 3]
    property int lenA: la.length
    property int na: 0
    onLaChanged: na++
    property list<string> lb: ["a", "b", "c"]
    property int lenB: lb.length
    property int nb: 0
    onLbChanged: nb++
    property list<int> lc: [1, 2, 3]
    property int lenC: lc.length
    property int nc: 0
    onLcChanged: nc++
    property list<var> ld: [1, "a"]
    property int lenD: ld.length
    property int nd: 0
    onLdChanged: nd++
    property list<QtObject> le: [QtObject { objectName: "a" }, QtObject { objectName: "b" }]
    property int lenE: le.length
    property int ne: 0
    onLeChanged: ne++
    property list<Item> lf: [Item { objectName: "a" }]
    property int lenF: lf.length
    property int nf: 0
    onLfChanged: nf++
    Component.onCompleted: {
        var n = na; la.length = 5; result += "a" + (na - n) + "," + lenA + "," + la.length + ";"
        n = nb; lb.length = 2; result += "b" + (nb - n) + "," + lenB + "," + lb.length + ";"
        function f() { lc.length = 4 }
        n = nc; f(); result += "c" + (nc - n) + "," + lenC + "," + lc.length + ";"
        n = nd; ld.length = 1; result += "d" + (nd - n) + "," + lenD + "," + ld.length + ";"
        n = ne; le.length = 1; result += "e" + (ne - n) + "," + lenE + "," + le.length + ";"
        n = nf; lf.length = 2; result += "f" + (nf - n) + "," + lenF + "," + lf.length + ";"
        var a = la; n = na; a.length = 1; result += "g" + (na - n) + "," + lenA + "," + la.length + ";"
    }
}
