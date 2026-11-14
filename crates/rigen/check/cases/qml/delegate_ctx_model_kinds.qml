// The rows of the other kinds of model (Qt 6.12): a JS array of objects, a `list<QtObject>`,
// the same list in a `var`, one object, a string, a list of strings, a JS object, undefined
// and null.
// qt: js count number:2;js name RefErr(name);js model.name undefined;js modelData.name string:b;js index number:1;js write string:W;typed count number:2;typed name string:b;typed model.name string:b;typed modelData.name string:b;typed index number:1;typed objectName string:;typed model.objectName string:;typedInVar count number:2;typedInVar name string:b;typedInVar model.name string:b;typedInVar modelData.name string:b;typedInVar index number:1;single count number:1;single name string:b;single model.name string:b;single modelData object;single modelData.name string:b;single index number:0;str count number:3;str modelData string:c;str index number:2;str model.modelData string:c;strs count number:2;strs modelData string:q;strs index number:1;strs model.modelData string:q;obj count number:0;obj FAILED TypeError: Cannot call method 'go' of null;undef count number:0;null count number:0
import QtQuick
import QtQml.Models
Item {
    id: root
    width: 200; height: 200
    property string result: ""
    property var seen: []
    function s(v) {
        if (v === undefined) return "undefined"
        if (v === null) return "null"
        if (typeof v === "object" || typeof v === "function") return typeof v
        return typeof v + ":" + v
    }
    function q(fn) { try { return s(fn()) } catch (e) { return ("" + e).replace(/ReferenceError: (\w+) is not defined/, "RefErr($1)").replace(/TypeError: Cannot read property '(\w+)' of undefined/, "NoProp($1)") } }
    function t(name, fn) { seen.push(name + " " + q(fn)) }
    QtObject { id: oa; property string name: "a"; property int n: 1 }
    QtObject { id: ob; property string name: "b"; property int n: 2 }
    property var jsobjs: [oa, ob]
    property list<QtObject> typed: [oa, ob]
    property var typedInVar: typed
    property var single: ob
    // a var holding a JS array of QObjects
    Repeater { id: r_js; model: root.jsobjs; Item { function go(tag) {
        root.t(tag + " name", () => name); root.t(tag + " model.name", () => model.name); root.t(tag + " modelData.name", () => modelData.name); root.t(tag + " index", () => index)
        root.t(tag + " write", () => { modelData.name = "W"; return ob.name }); ob.name = "b" } } }
    Repeater { id: r_typed; model: root.typed; Item { function go(tag) {
        root.t(tag + " name", () => name); root.t(tag + " model.name", () => model.name); root.t(tag + " modelData.name", () => modelData.name); root.t(tag + " index", () => index)
        root.t(tag + " objectName", () => objectName); root.t(tag + " model.objectName", () => model.objectName) } } }
    Repeater { id: r_tv; model: root.typedInVar; Item { function go(tag) {
        root.t(tag + " name", () => name); root.t(tag + " model.name", () => model.name); root.t(tag + " modelData.name", () => modelData.name); root.t(tag + " index", () => index) } } }
    // one QObject as the model
    Repeater { id: r_single; model: root.single; Item { function go(tag) {
        root.t(tag + " name", () => name); root.t(tag + " model.name", () => model.name); root.t(tag + " modelData", () => modelData); root.t(tag + " modelData.name", () => modelData.name); root.t(tag + " index", () => index) } } }
    // a string and a list of strings
    Repeater { id: r_str; model: "abc"; Item { function go(tag) { root.t(tag + " modelData", () => modelData); root.t(tag + " index", () => index); root.t(tag + " model.modelData", () => model.modelData) } } }
    Repeater { id: r_strs; model: ["p", "q"]; Item { function go(tag) { root.t(tag + " modelData", () => modelData); root.t(tag + " index", () => index); root.t(tag + " model.modelData", () => model.modelData) } } }
    // model that is an object with no rows
    Repeater { id: r_obj; model: ({ name: "o" }); Item { function go(tag) { root.t(tag + " modelData", () => modelData); root.t(tag + " name", () => name); root.t(tag + " index", () => index) } } }
    Repeater { id: r_undef; model: undefined; Item { } }
    Repeater { id: r_null; model: null; Item { } }
    function run() {
        var all = { js: r_js, typed: r_typed, typedInVar: r_tv, single: r_single, str: r_str, strs: r_strs, obj: r_obj }
        for (var k in all) {
            t(k + " count", () => all[k].count)
            try { all[k].itemAt(all[k].count - 1).go(k) } catch (e) { seen.push(k + " FAILED " + e) }
        }
        t("undef count", () => r_undef.count); t("null count", () => r_null.count)
        result = seen.join(";")
    }
    Component.onCompleted: Qt.callLater(run)
}
