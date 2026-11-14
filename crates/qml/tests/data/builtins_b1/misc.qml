import QtQuick
Item {
    id: root
    property QtObject q: QtObject { property int a: 1 }
    property real w2: 61.5
    property int i2: 3000000
    property var arr: [1]
    signal sig(int a)
    function named2(a, b) { return a }
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    Component.onCompleted: {
        var FT = Function.prototype.toString
        t("F1", function() { return FT.call(function named(a) { return a }) })
        t("F2", function() { return [FT.call(function() {}), FT.call((a) => a), FT.call(Math.max), FT.call(function() {}.bind(null)), FT.call(root.named2), FT.call(function*g() {}), FT.call(class K {}), FT.call({ m() {} }.m), FT.call(Object), FT.call(Array.prototype.push), FT.call(root.sig), FT.call(root.widthChanged), FT.call(Symbol), FT.call(Proxy)].join(" | ") })
        t("F3", function() { var f = function() {}; var g = function inner() {}; var o = { k: function() {}, ["c" + 1]: function() {}, get x() { return 1 } }; var h = g.bind(null); return [String(f), String(g), String(o.k), String(o.c1), String(Object.getOwnPropertyDescriptor(o, "x").get), String(h), f.name, o.k.name].join(" | ") })
        t("F4", function() { function nm() {} Object.defineProperty(nm, "name", { value: "other" }); var u = function orig() {}; Object.defineProperty(u, "name", { value: 5 }); return String(nm) + " | " + String(u) + " | " + (nm + "") + " | " + nm.toString() })
        t("F5", function() { return [FT.call(new Function("a", "return a")), String(new Function()), String(root.t), String(Qt.rgba), String(console.log), String(JSON.parse), String("".arg), String(parseInt)].join(" | ") })
        t("F6", function() { try { return FT.call({}) } catch (e) { return e.name + ": " + e.message } })
        t("F7", function() { return [FT.call(new Proxy(function pf() {}, {})), String(function() {}.bind(null).bind(null)), String(function a$_1() {}), String(function \u0061bc() {})].join(" | ") })
        var cn = function(x) { try { root.widthChanged.connect(x, function() {}); return "ok" } catch (e) { return e.name + ": " + e.message } }
        t("C1", function() { return [cn(null), cn(5), cn(undefined), cn("s"), cn(true), cn({}), cn(root), cn([]), cn(Symbol()), cn(function() {}), cn(root.q), cn(root.width)].join(" | ") })
        t("C2", function() { try { root.widthChanged.connect(null, null) } catch (e) { return e.name + ": " + e.message } return "ok" })
        t("C3", function() { try { root.widthChanged.connect({}, 5) } catch (e) { return e.name + ": " + e.message } return "ok" })
        t("C4", function() { try { root.widthChanged.disconnect(null, function() {}) } catch (e) { return e.name + ": " + e.message } return "ok" })
        t("C5", function() { try { root.widthChanged.disconnect(5, function() {}) } catch (e) { return e.name + ": " + e.message } return "ok" })
        t("C6", function() { try { root.sig.connect(null, function() {}, 1) } catch (e) { return e.name + ": " + e.message } return "ok" })
        t("C7", function() { try { root.widthChanged.connect() } catch (e) { return e.name + ": " + e.message } return "ok" })
        t("C8", function() { try { root.widthChanged.connect(5) } catch (e) { return e.name + ": " + e.message } return "ok" })
        t("C9", function() { try { root.widthChanged.disconnect({}, function() {}) } catch (e) { return e.name + ": " + e.message } return "ok" })
        t("C10", function() { var n = 0; var o = { v: 7 }; root.sig.connect(o, function() { n = this.v }); root.sig(1); return n })
        t("Z1", function() { return (Object.freeze(q) === q) + " " + (Object.seal(q) === q) + " " + (Object.preventExtensions(q) === q) + " " + (Object.freeze(root) === root) })
        t("Z2", function() { Object.freeze(q); q.a = 5; return q.a + " " + Object.isFrozen(q) + " " + Object.isSealed(q) + " " + Object.isExtensible(q) })
        t("Z3", function() { q.b1new = 1; return q.b1new })
        t("Z4", function() { return Reflect.preventExtensions(q) + " " + Object.isExtensible(q) })
        t("Z5", function() { return [Object.freeze(5), Object.seal("s"), Object.preventExtensions(true), Object.freeze(null), Object.freeze(undefined)].join() + " " + (Object.freeze(root.arr) === root.arr) })
        t("Z6", function() { var c = Qt.rgba(1, 0, 0, 1); return String(Object.freeze(c)) + " " + Object.isFrozen(c) + " " + String(Object.seal(Qt.point(1, 2))) })
        t("Z7", function() { "use strict"; Object.freeze(q); try { q.a = 9; return "set " + q.a } catch (e) { return e.name } })
        t("Z8", function() { try { Object.defineProperty(q, "a", { value: 3 }); return "ok " + q.a } catch (e) { return e.name + ": " + e.message } })
        t("G1", function() { return "%1".arg(Math.round(1234567.8)) + " " + "%1".arg(parseInt("1234567")) + " " + "%1".arg(4294967295) })
        t("G2", function() { return "%1".arg(0.00005) + " " + "%1".arg(100000.5) })
        t("G3", function() { return "%1".arg(false) + " " + "%09 %9".arg("x") + " " + "%001".arg("x") })
        t("G4", function() { return "%1".arg(Symbol()) })
        t("G5", function() { return ["%1".arg(1234567), "%1".arg(-2147483648), "%1".arg(2147483647), "%1".arg(2147483648), "%1".arg(-2147483649), "%1".arg(1e6), "%1".arg(1e21), "%1".arg(1e15), "%1".arg(123456789012), "%1".arg(0x7fffffff), "%1".arg(1000000.0), "%1".arg(999999), "%1".arg(999999.5), "%1".arg(-0), "%1".arg(0)].join(" ") })
        t("G6", function() { return ["%1".arg(0.1), "%1".arg(1.5), "%1".arg(1 / 3), "%1".arg(123456.7), "%1".arg(1234567.8), "%1".arg(0.0001), "%1".arg(0.00001234), "%1".arg(1e-7), "%1".arg(NaN), "%1".arg(Infinity), "%1".arg(-Infinity), "%1".arg(-1.5), "%1".arg(2.5e-5), "%1".arg(99999.95), "%1".arg(999999.4), "%1".arg(0.000099999), "%1".arg(1.0000005), "%1".arg(12345.65), "%1".arg(5e-324), "%1".arg(1.7976931348623157e308)].join(" ") })
        t("G7", function() { return ["%1".arg(Number("1234567")), "%1".arg(root.width * 20000 + 5000000), "%1".arg(new Date(86400000 * 365).getTime()), "%1".arg(root.i2), "%1".arg(root.w2 * 100000), "%1".arg(Math.floor(1234567.8)), "%1".arg(Math.ceil(1234567.2)), "%1".arg(Math.abs(-1234567)), "%1".arg(Math.max(1234567, 2)), "%1".arg(1234567 + 1), "%1".arg(1234567 * 2), "%1".arg(2469134 / 2), "%1".arg(1234567 | 0), "%1".arg(+"1234567"), "%1".arg("1234567".length + 1234560), "%1".arg(JSON.parse("1234567")), "%1".arg([1234567][0]), "%1".arg(1234567.0), "%1".arg(12345678 % 10000000), "%1".arg(-(-1234567)), "%1".arg(1234567 >>> 0), "%1".arg(4294967295 >>> 0), "%1".arg(1e3 * 1e3), "%1".arg(Math.trunc(1234567.5)), "%1".arg(Math.pow(10, 6)), "%1".arg(10 ** 6), "%1".arg(Number.MAX_SAFE_INTEGER), "%1".arg(Math.min(1234567, 2234567)), "%1".arg(~~1234567.5), "%1".arg(1234567.5 - 0.5), "%1".arg(parseFloat("1234567")), "%1".arg(Number(1234567)), "%1".arg(new Number(1234567)), "%1".arg(Math.sqrt(1524155677489)), "%1".arg(Date.now() > 1e12 ? 1 : 0)].join(" ") })
        t("G8", function() { return ["%1".arg(true), "%1".arg(null), "%1".arg(undefined), "%1".arg([1, 2]), "%1".arg({}), "%1".arg(new Date(0)), "%1".arg("s"), "%1".arg(""), "%1".arg(/a/g), "%1".arg(function f() {}), "%1".arg(root.q) === String(root.q), "%1".arg(Qt.rgba(1, 0, 0, 1)), "%1".arg(new String("ws")), "%1".arg(new Boolean(true)), "%1".arg({ toString: function() { return "ts" } }), "%1".arg({ valueOf: function() { return 5 } }), "%1".arg(Qt.point(1, 2))].join("|") })
        t("G9", function() { return ["%1%2".arg("a"), "%2%1".arg("a"), "%1 %1".arg("a"), "%3 %5".arg("a"), "%09 %9".arg("x"), "%001".arg("x"), "%01".arg("x"), "%0".arg("x"), "%00".arg("x"), "%10 %1".arg("x"), "%99".arg("x"), "%100".arg("x"), "%999".arg("x"), "%".arg("x"), "%%1".arg("x"), "%1%".arg("x"), "% 1".arg("x"), "%-1".arg("x"), "%a".arg("x"), "%1a%2".arg("%2").arg("y"), "%2 %1 %3".arg("a").arg("b"), "%12".arg("x"), "%012".arg("x"), "%1%1%2".arg(1).arg(2), "é%1ü".arg("ß"), "%L1".arg("x"), "%L1".arg(1234567.5), "%L2 %1".arg("x"), "%l1".arg("x"), "%1".arg("%1").arg("z"), "%05".arg("x"), "%5 %05 %005".arg("x"), "%١".arg("x"), "x".arg("y")].join("|") })
        t("G10", function() { try { return "%1".arg() } catch (e) { return e.name + ": " + e.message } })
        t("G11", function() { try { return "%1 %2".arg("a", "b") } catch (e) { return e.name + ": " + e.message } })
        t("G12", function() { try { return "%1".arg({ toString: function() { throw new RangeError("bad") } }) } catch (e) { return e.name + ": " + e.message } })
        t("G13", function() { return [String.prototype.arg.length, String.prototype.arg.name, String.prototype.arg.call(5, 1), String.prototype.arg.call("%1", 1.5), new String("%1").arg(7)].join("|") })
        t("G14", function() { try { return String.prototype.arg.call(null, 1) } catch (e) { return e.name + ": " + e.message } })
        Qt.quit()
    }
}
