// A delegate's own properties named like the context's (Qt 6.12): a declared `modelData`,
// `index` or role hides the context's and is not set from the row; `required` ones are; a
// required property in a child of the delegate or one no role fills.
// errors: 4 (Qt warns of the same four: Required property name, extra was not initialized)
// qt: plainDecl count number:2;plainDecl modelData undefined;plainDecl index number:-5;plainDecl model undefined;plainDeclLm count number:2;plainDeclLm name string:;plainDeclLm modelData undefined;plainDeclLm n number:2;plainDeclLm index number:1;plainDeclLm model.name string:b;boundDecl count number:2;boundDecl modelData string:bound;boundDecl index number:42;boundDecl model.modelData string:y;boundDecl model.index number:1;mixed count number:2;mixed index number:1;mixed name string:own;mixed n RefErr(n);mixed model string:undefined;reqOther count number:2;reqOther FAILED TypeError: Cannot call method 'go' of null;reqChild count number:2;reqChild FAILED TypeError: Cannot call method 'go' of null;inl count number:2;inl string:b/1;inl n RefErr(n)
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function s(v) {
        if (v === undefined) return "undefined"
        if (v === null) return "null"
        if (typeof v === "object" || typeof v === "function") return typeof v
        return typeof v + ":" + v
    }
    function q(fn) { try { return s(fn()) } catch (e) { return ("" + e).replace(/ReferenceError: (\w+) is not defined/, "RefErr($1)") } }
    function t(name, fn) { seen.push(name + " " + q(fn)) }
    ListModel { id: lm; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }
    // declared, not required, unbound
    Repeater { id: plainDecl; model: ["x", "y"]; Item {
        property var modelData
        property int index: -5
        property var model
        function go(tag) { root.t(tag + " modelData", () => modelData); root.t(tag + " index", () => index); root.t(tag + " model", () => model) }
    } }
    Repeater { id: plainDeclLm; model: lm; Item {
        property string name
        property var modelData
        function go(tag) { root.t(tag + " name", () => name); root.t(tag + " modelData", () => modelData); root.t(tag + " n", () => n); root.t(tag + " index", () => index); root.t(tag + " model.name", () => model.name) }
    } }
    // declared with a binding
    Repeater { id: boundDecl; model: ["x", "y"]; Item {
        property var modelData: "bound"
        property int index: 40 + 2
        function go(tag) { root.t(tag + " modelData", () => modelData); root.t(tag + " index", () => index); root.t(tag + " model.modelData", () => model.modelData); root.t(tag + " model.index", () => model.index) }
    } }
    // required with others not required
    Repeater { id: mixed; model: lm; Item {
        required property int index
        property string name: "own"
        function go(tag) { root.t(tag + " index", () => index); root.t(tag + " name", () => name); root.t(tag + " n", () => n); root.t(tag + " model", () => typeof model) }
    } }
    // a required property that is no role, set by nobody: delegate fails; one with a default binding
    Repeater { id: reqOther; model: 2; Item { required property int index; required property string extra; function go(tag) { root.t(tag, () => index) } } }
    // required in a child of the delegate, not its root
    Repeater { id: reqChild; model: lm; Item {
        function go(tag) { root.t(tag + " name", () => name); root.t(tag + " index", () => index); root.t(tag + " kid", () => kid.name) }
        Item { id: kid; required property string name }
    } }
    // component from a file-like inline type with required
    component Row2: Item { required property string name; required property int index; function go(tag) { root.t(tag, () => name + "/" + index); root.t(tag + " n", () => n) } }
    Repeater { id: inl; model: lm; Row2 {} }
    function run() {
        var all = { plainDecl: plainDecl, plainDeclLm: plainDeclLm, boundDecl: boundDecl, mixed: mixed, reqOther: reqOther, reqChild: reqChild, inl: inl }
        for (var k in all) { t(k + " count", () => all[k].count); try { all[k].itemAt(1).go(k) } catch (e) { seen.push(k + " FAILED " + e) } }
        result = seen.join(";")
    }
    Component.onCompleted: Qt.callLater(run)
}
