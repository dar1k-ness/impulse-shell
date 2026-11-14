import QtQuick
Item {
    id: root
    signal sig(int a)
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    Component.onCompleted: {
        t("N1", function() { var b = function a() {}.bind(null); return b.name + "|" + String(b) + "|" + root.sig.name + "|" + Qt.rgba.name + "|" + root.widthChanged.name + "|" + typeof Qt.rgba })
        t("N2", function() { function a() {} delete a.name; return String(a) + "|" + a.name })
        t("N3", function() { function a() {} Object.defineProperty(a, "name", { get: function() { return "acc" } }); return String(a) })
        t("N4", function() { function a() {} Object.defineProperty(a, "name", { value: "" }); var b = function() {}; Object.defineProperty(b, "name", { value: "x y" }); return String(a) + "|" + String(b) })
        t("N5", function() { function a() {} a.toString = function() { return "own" }; return String(a) + "|" + (a + "") + "|" + Function.prototype.toString.call(a) })
        t("N6", function() { var o = { get x() { return 1 }, set x(v) {} }; var d = Object.getOwnPropertyDescriptor(o, "x"); return d.get.name + "|" + d.set.name + "|" + String(d.set) })
        t("N7", function() { var s = Symbol("d"); var o = { [s]: function() {} }; return o[s].name + "|" + String(o[s]) })
        t("N8", function() { var p = new Proxy(function pf() {}, { get: function(t, k) { return k === "name" ? "trap" : t[k] } }); return String(p) + "|" + Function.prototype.toString.call(p) })
        t("N9", function() { return String(Function.prototype) + "|" + String(Object.getPrototypeOf(function*() {})) + "|" + String(function*() {}) + "|" + String(() => 1) + "|" + String(Function) + "|" + String(Object.getOwnPropertyDescriptor(Map.prototype, "size").get) + "|" + String(Promise.resolve) + "|" + String(Array.prototype[Symbol.iterator]) + "|" + String(RegExp.prototype[Symbol.replace])+ "|" + String(Symbol.prototype[Symbol.toPrimitive]) })
        Qt.quit()
    }
}
