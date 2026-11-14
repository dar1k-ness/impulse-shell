// A required property of a delegate's type set where the type is used (Qt 6.12): the value
// given there stays, the row does not replace it, and the context's names are gone all the
// same.
// errors: 2 (Qt warns twice too: ReferenceError: index is not defined)
// qt: setAtUse name string:x;setAtUse n RefErr(n);setAtUse index RefErr(index);setAtUse model string:undefined;boundAtUse name string:;boundAtUse n RefErr(n);boundAtUse index RefErr(index);boundAtUse model string:undefined;unset name string:b;unset n RefErr(n);unset index RefErr(index);unset model string:undefined
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function q(fn) { try { var v = fn(); return typeof v + ":" + v } catch (e) { return ("" + e).replace(/ReferenceError: (\w+) is not defined/, "RefErr($1)") } }
    function t(name, fn) { seen.push(name + " " + q(fn)) }
    ListModel { id: lm; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }
    component Tab: Item { required property string name; function go(tag) { root.t(tag + " name", () => name); root.t(tag + " n", () => n); root.t(tag + " index", () => index); root.t(tag + " model", () => typeof model) } }
    Repeater { id: setAtUse; model: lm; Tab { name: "x" } }
    Repeater { id: boundAtUse; model: lm; Tab { name: "i" + index } }
    Repeater { id: unset; model: lm; Tab { } }
    function run() {
        var all = { setAtUse: setAtUse, boundAtUse: boundAtUse, unset: unset }
        for (var k in all) { try { all[k].itemAt(1).go(k) } catch (e) { seen.push(k + " FAILED " + e) } }
        result = seen.join(";")
    }
    Component.onCompleted: Qt.callLater(run)
}
