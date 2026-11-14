// A read, a write, a delete and a call on `undefined` and `null`, in Qt 6.12's words (QV4's
// Runtime::StoreProperty, LoadProperty, DeleteProperty, CallProperty): sloppy and strict code
// word a write differently, and a call names the method.
// qt: undefined.foo = TypeError: Value is undefined and could not be converted to an object;null.foo = TypeError: Value is null and could not be converted to an object;undefined[k] = TypeError: Value is undefined and could not be converted to an object;null[k] = TypeError: Value is null and could not be converted to an object;undefined[0] = TypeError: Value is undefined and could not be converted to an object;null[0] = TypeError: Value is null and could not be converted to an object;undefined[sym] = TypeError: Value is undefined and could not be converted to an object;literal undefined TypeError: Value is undefined and could not be converted to an object;literal null TypeError: Value is null and could not be converted to an object;prop undefined TypeError: Value is undefined and could not be converted to an object;prop null TypeError: Value is null and could not be converted to an object;free prop undefined TypeError: Value is undefined and could not be converted to an object;free prop null TypeError: Value is null and could not be converted to an object;member null TypeError: Value is null and could not be converted to an object;member undefined TypeError: Value is undefined and could not be converted to an object;member missing TypeError: Value is undefined and could not be converted to an object;deep TypeError: Cannot read property 'b' of null;call null TypeError: Value is null and could not be converted to an object;compound undefined TypeError: Cannot read property 'foo' of undefined;compound null TypeError: Cannot read property 'foo' of null;incr undefined TypeError: Cannot read property 'foo' of undefined;incr null TypeError: Cannot read property 'foo' of null;compound [k] TypeError: Cannot read property 'key' of undefined;destructure TypeError: Value is undefined and could not be converted to an object;for in TypeError: Value is undefined and could not be converted to an object;rhs first rhs|Value is undefined and could not be converted to an object;key first keyrhs|Value is undefined and could not be converted to an object;delete undefined TypeError: Value is undefined and could not be converted to an object;delete null TypeError: Value is null and could not be converted to an object;read undefined TypeError: Cannot read property 'foo' of undefined;read null TypeError: Cannot read property 'foo' of null;read [k] TypeError: Cannot read property 'key' of undefined;call undefined TypeError: Cannot call method 'foo' of undefined;strict undefined TypeError: Type error;strict null TypeError: Type error;strict [k] TypeError: Type error;strict [0] TypeError: Type error;strict compound TypeError: Cannot read property 'foo' of undefined;strict delete TypeError: Value is undefined and could not be converted to an object;strict delete null [k] TypeError: Value is null and could not be converted to an object;strict prop TypeError: Type error;strict fn TypeError: Type error;strict rhs first rhs|Type error;delete [k] TypeError: Value is undefined and could not be converted to an object;delete null [0] TypeError: Value is null and could not be converted to an object;delete member TypeError: Value is null and could not be converted to an object;call null TypeError: Cannot call method 'foo' of null;call [k] TypeError: Cannot read property 'key' of undefined;call member TypeError: Cannot call method 'foo' of null;call free TypeError: Cannot call method 'foo' of undefined;call optional undefined;call args first arg|Cannot call method 'foo' of undefined;strict call TypeError: Cannot read property 'foo' of undefined;strict read TypeError: Cannot read property 'foo' of undefined;id missing prop TypeError: Value is undefined and could not be converted to an object;this strict TypeError: Type error;parent null TypeError: Value is null and could not be converted to an object;
import QtQuick
Item {
    id: root
    property string result: ""
    property var nothing
    property var nul: null
    property var obj: ({ a: null, u: undefined })
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + e.message }
        result += what + " " + r + ";"
    }
    function giveNull() { return null }
    Component.onCompleted: {
        t("undefined.foo =", () => { var u; u.foo = 1 })
        t("null.foo =", () => { var n = null; n.foo = 1 })
        t("undefined[k] =", () => { var u; var k = "key"; u[k] = 1 })
        t("null[k] =", () => { var n = null; var k = "key"; n[k] = 1 })
        t("undefined[0] =", () => { var u; u[0] = 1 })
        t("null[0] =", () => { var n = null; n[0] = 1 })
        t("undefined[sym] =", () => { var u; u[Symbol.iterator] = 1 })
        t("literal undefined", () => { undefined.foo = 1 })
        t("literal null", () => { null.foo = 1 })
        t("prop undefined", () => { root.nothing.foo = 1 })
        t("prop null", () => { root.nul.foo = 1 })
        t("free prop undefined", () => { nothing.foo = 1 })
        t("free prop null", () => { nul.foo = 1 })
        t("member null", () => { obj.a.foo = 1 })
        t("member undefined", () => { obj.u.foo = 1 })
        t("member missing", () => { obj.zz.foo = 1 })
        t("deep", () => { obj.a.b.c = 1 })
        t("call null", () => { giveNull().foo = 1 })
        t("compound undefined", () => { var u; u.foo += 1 })
        t("compound null", () => { var n = null; n.foo += 1 })
        t("incr undefined", () => { var u; u.foo++ })
        t("incr null", () => { var n = null; n.foo++ })
        t("compound [k]", () => { var u; var k = "key"; u[k] += 1 })
        t("destructure", () => { var u; [u.foo] = [1] })
        t("for in", () => { var u; for (u.foo in { a: 1 }) {} })
        t("rhs first", () => { var u; var s = ""; try { u.foo = (s += "rhs", 1) } catch (e) { return s + "|" + e.message } })
        t("key first", () => { var u; var s = ""; try { u[(s += "key", "k")] = (s += "rhs", 1) } catch (e) { return s + "|" + e.message } })
        t("delete undefined", () => { var u; delete u.foo })
        t("delete null", () => { var n = null; delete n.foo })
        t("read undefined", () => { var u; return u.foo })
        t("read null", () => { var n = null; return n.foo })
        t("read [k]", () => { var u; var k = "key"; return u[k] })
        t("call undefined", () => { var u; return u.foo() })
        t("strict undefined", () => { "use strict"; var u; u.foo = 1 })
        t("strict null", () => { "use strict"; var n = null; n.foo = 1 })
        t("strict [k]", () => { "use strict"; var u; var k = "key"; u[k] = 1 })
        t("strict [0]", () => { "use strict"; var n = null; n[0] = 1 })
        t("strict compound", () => { "use strict"; var u; u.foo += 1 })
        t("strict delete", () => { "use strict"; var u; delete u.foo })
        t("strict delete null [k]", () => { "use strict"; var n = null; var k = "key"; delete n[k] })
        t("strict prop", () => { "use strict"; root.nothing.foo = 1 })
        t("strict fn", () => { return (function() { "use strict"; var u; u.foo = 1 })() })
        t("strict rhs first", () => { "use strict"; var u; var s = ""; try { u.foo = (s += "rhs", 1) } catch (e) { return s + "|" + e.message } })
        t("delete [k]", () => { var u; var k = "key"; delete u[k] })
        t("delete null [0]", () => { var n = null; delete n[0] })
        t("delete member", () => { delete obj.a.foo })
        t("call null", () => { var n = null; return n.foo() })
        t("call [k]", () => { var u; var k = "key"; return u[k]() })
        t("call member", () => { return obj.a.foo() })
        t("call free", () => { return nothing.foo() })
        t("call optional", () => { var u; return u?.foo() })
        t("call args first", () => { var u; var s = ""; try { u.foo((s += "arg", 1)) } catch (e) { return s + "|" + e.message } })
        t("strict call", () => { "use strict"; var u; return u.foo() })
        t("strict read", () => { "use strict"; var u; return u.foo })
        t("id missing prop", () => { root.zz.foo = 1 })
        t("this strict", () => { "use strict"; (function() { this.foo = 1 })() })
        t("parent null", () => { root.parent.foo = 1 })
    }
}
