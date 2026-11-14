import QtQuick
Item {
    id: root
    function t(n, f) { var r; try { r = f() } catch (e) { r = "THROW " + e.name + ": " + e.message } console.log(n + " => " + r) }
    Component.onCompleted: {
        t("N3a", function() { function a() {} Object.defineProperty(a, "name", { get: function() { return "acc" } }); return a.name + "|" + String(a) + "|" + Function.prototype.toString.call(a) + "|" + (a + "") })
        t("N3b", function() { var a = function() {}; Object.defineProperty(a, "name", { get: function() { return "acc" } }); return a.name + "|" + String(a) })
        t("N3c", function() { function a() {} Object.defineProperty(a, "name", { value: "v" }); return a.name + "|" + String(a) })
        t("N3d", function() { function a() {} Object.defineProperty(a, "name", { value: 5 }); return a.name + "|" + String(a) })
        t("N3e", function() { function a() {} delete a.name; return a.name + "|" + String(a) })
        t("N3f", function() { class A { static toString() { return "cls" } } return String(A) + "|" + (A + "") + "|" + `${A}` })
        t("N3g", function() { function a() {} a.valueOf = function() { return 7 }; return String(a) + "|" + (a + 1) + "|" + (a * 2) })
        t("N3h", function() { function a() {} a[Symbol.toPrimitive] = function(h) { return h }; return String(a) + "|" + (a + "") + "|" + `${a}` })
    }
}
