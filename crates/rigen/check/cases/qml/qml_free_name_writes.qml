// A write to a name without an object (`foo = 1`), as in Qt 6.12 (QV4's
// QQmlContextWrapper::virtualPut): a name declared nowhere is an error in a binding, a
// handler, a function, a `.js` file and a `.pragma library` alike, and makes no global; a
// `var`, a `let` and a closure's variable take the write; a property in scope takes it; an
// id is no lvalue; a method, a signal, a handler and a read-only property refuse it.
// qt: undeclared Error: Invalid write to global property "foo";undeclared strict Error: Invalid write to global property "foo2";compound ReferenceError: foo3 is not defined;increment ReferenceError: foo4 is not defined;var and let 3;closure 3;hoisted var 1;property 8;cpp property 12;child scope 11;child own 5;child undeclared Error: Invalid write to global property "cfoo";child method TypeError: Cannot assign to read-only property "ping";child id Error: left-hand side of assignment operator is not an lvalue;id Error: left-hand side of assignment operator is not an lvalue;method TypeError: Cannot assign to read-only property "ping";signal TypeError: Cannot assign to read-only property "poked";handler name TypeError: Cannot assign to read-only property "poked";changed TypeError: Cannot assign to read-only property "declaredChanged";cpp method TypeError: Cannot assign to read-only property "forceActiveFocus";cpp changed TypeError: Cannot assign to read-only property "widthChanged";cpp read-only TypeError: Cannot assign to read-only property "childrenRect";group TypeError: Cannot assign to read-only property "anchors";destroy Error: Invalid write to global property "destroy";toString Error: Invalid write to global property "toString";type Error: Invalid write to global property "Item";namespace Error: Invalid write to global property "Plain";Math Error: Invalid write to global property "Math";undefined Error: Invalid write to global property "undefined";NaN Error: Invalid write to global property "NaN";Qt Error: Invalid write to global property "Qt";console Error: Invalid write to global property "console";parseInt Error: Invalid write to global property "parseInt";for in Error: Invalid write to global property "fk";for of Error: Invalid write to global property "fo";destructure array Error: Invalid write to global property "da";destructure object Error: Invalid write to global property "dob";function this number;bound Error: Invalid write to global property "bfoo";handler Error: Invalid write to global property "hfoo";js var 5;js let 6;js undeclared Error: Invalid write to global property "jsfoo";js undeclared strict Error: Invalid write to global property "jsfoo2";js compound ReferenceError: jsfoo3 is not defined;js local 3;js closure 3;js for in Error: Invalid write to global property "jk";js destructure Error: Invalid write to global property "jd";js this Error: Invalid write to global property "jt";js Math Error: Invalid write to global property "Math";js undefined Error: Invalid write to global property "undefined";js function number;js namespace 44;js namespace new Error: Invalid write to global property "nn";library var 5;library let 6;library undeclared Error: Invalid write to global property "libfoo";library undeclared strict Error: Invalid write to global property "libfoo2";library compound ReferenceError: libfoo3 is not defined;library local 3;library closure 3;library for in Error: Invalid write to global property "lk";library destructure Error: Invalid write to global property "ld";library this Error: Invalid write to global property "lt";library Math Error: Invalid write to global property "Math";library function number;library read back undefined,undefined,undefined,undefined;library seen from qml undefined,undefined;library namespace 44;library namespace new Error: Invalid write to global property "nn";
import QtQuick
import "ev_write_plain.js" as Plain
import "ev_write_lib.js" as Lib
Rectangle {
    id: root
    property int declared: 7
    property string result: ""
    property var bound: { try { bfoo = 1; return typeof bfoo } catch (e) { return e.name + ": " + e.message } }
    signal poked()
    function ping() { return "pong" }
    Item { id: inner; property int innerProp: 1
        function childWrite() { declared = 11; return root.declared }
        function childOwn() { innerProp = 5; return innerProp }
        function childUndeclared() { cfoo = 1; return typeof cfoo }
        function childMethod() { ping = 3; return typeof ping }
        function childId() { root = 3; return 0 }
    }
    onPoked: { try { hfoo = 1; result += "handler " + typeof hfoo + ";" } catch (e) { result += "handler " + e.name + ": " + e.message + ";" } }
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + e.message }
        result += what + " " + r + ";"
    }
    function run() {
        t("undeclared", () => { foo = 1; return typeof foo })
        t("undeclared strict", () => { "use strict"; foo2 = 1; return typeof foo2 })
        t("compound", () => { foo3 += 1; return typeof foo3 })
        t("increment", () => { foo4++; return typeof foo4 })
        t("var and let", () => { var a; a = 1; let b; b = 2; return a + b })
        t("closure", () => { var c = 0; (function() { c = 3 })(); return c })
        t("hoisted var", () => { hv = 1; var hv; return hv })
        t("property", () => { declared = 8; return declared })
        t("cpp property", () => { width = 12; return width })
        t("child scope", () => inner.childWrite())
        t("child own", () => inner.childOwn())
        t("child undeclared", () => inner.childUndeclared())
        t("child method", () => inner.childMethod())
        t("child id", () => inner.childId())
        t("id", () => { inner = 1; return typeof inner })
        t("method", () => { ping = 1; return typeof ping })
        t("signal", () => { poked = 1; return typeof poked })
        t("handler name", () => { onPoked = 1; return 0 })
        t("changed", () => { declaredChanged = 1; return 0 })
        t("cpp method", () => { forceActiveFocus = 1; return 0 })
        t("cpp changed", () => { widthChanged = 1; return 0 })
        t("cpp read-only", () => { childrenRect = 1; return typeof childrenRect })
        t("group", () => { anchors = 1; return 0 })
        t("destroy", () => { destroy = 1; return 0 })
        t("toString", () => { toString = 1; return 0 })
        t("type", () => { Item = 1; return typeof Item })
        t("namespace", () => { Plain = 1; return typeof Plain })
        t("Math", () => { Math = 1; return typeof Math })
        t("undefined", () => { undefined = 1; return typeof undefined })
        t("NaN", () => { NaN = 1; return String(NaN) })
        t("Qt", () => { Qt = 1; return typeof Qt })
        t("console", () => { console = 1; return typeof console })
        t("parseInt", () => { parseInt = 1; return typeof parseInt })
        t("for in", () => { for (fk in {a: 1}) {} return typeof fk })
        t("for of", () => { for (fo of [1]) {} return typeof fo })
        t("destructure array", () => { [da] = [1]; return typeof da })
        t("destructure object", () => { ({dob} = {dob: 1}); return typeof dob })
        t("function this", () => (function() { this.ft = 1; return typeof this.ft })())
        t("bound", () => bound)
        poked()
        t("js var", () => Plain.setG())
        t("js let", () => Plain.setGl())
        t("js undeclared", () => Plain.undeclared())
        t("js undeclared strict", () => Plain.undeclaredStrict())
        t("js compound", () => Plain.compound())
        t("js local", () => Plain.local())
        t("js closure", () => Plain.closure())
        t("js for in", () => Plain.forIn())
        t("js destructure", () => Plain.destructure())
        t("js this", () => Plain.thisWrite())
        t("js Math", () => Plain.builtin())
        t("js undefined", () => Plain.undef())
        t("js function", () => Plain.fnDecl())
        t("js namespace", () => { Plain.g = 44; return Plain.g })
        t("js namespace new", () => { Plain.nn = 44; return Plain.nn })
        t("library var", () => Lib.setG())
        t("library let", () => Lib.setGl())
        t("library undeclared", () => Lib.undeclared())
        t("library undeclared strict", () => Lib.undeclaredStrict())
        t("library compound", () => Lib.compound())
        t("library local", () => Lib.local())
        t("library closure", () => Lib.closure())
        t("library for in", () => Lib.forIn())
        t("library destructure", () => Lib.destructure())
        t("library this", () => Lib.thisWrite())
        t("library Math", () => Lib.builtin())
        t("library function", () => Lib.fnDecl())
        t("library read back", () => Lib.readBack())
        t("library seen from qml", () => [typeof libfoo, typeof lk])
        t("library namespace", () => { Lib.g = 44; return Lib.g })
        t("library namespace new", () => { Lib.nn = 44; return Lib.nn })
    }
    Component.onCompleted: run()
}
