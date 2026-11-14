import QtQuick
Item {
    function t(n, f) { try { console.log(n + " => " + f()) } catch (e) { console.log(n + " => THROW " + e) } }
    function k(o) { return Object.getOwnPropertyNames(o).join(",") + ";" + Object.getOwnPropertySymbols(o).map(String).join(",") }
    Component.onCompleted: {
        t("K1", function(){ return k(function f(a, b) {}) })
        t("K2", function(){ return k(() => 1) })
        t("K3", function(){ return k(class C { static s() {} m() {} }) })
        t("K5", function(){ return k(Math.max) })
        t("K15", function(){ var f = function f(a) {}; f.x = 1; return k(f) })
        t("K16", function(){ var f = function f(a) {}; delete f.name; f.name = 2; return k(f) })
        t("K17", function(){ return k(function* g() {}) })
        t("K24", function(){ return JSON.stringify(Array.prototype[Symbol.unscopables]) + Object.getPrototypeOf(Array.prototype[Symbol.unscopables]) + JSON.stringify(Object.getOwnPropertyDescriptor(Array.prototype, Symbol.unscopables)) })
        t("K26", function(){ return k({ m() {} }.m) + "|" + k(Object.getOwnPropertyDescriptor({ get p() { return 1 } }, "p").get) })
        t("K28", function(){ return Object.getOwnPropertyNames(Set.prototype) })
        t("K29", function(){ return Object.getOwnPropertyNames(new Error("x")) })
    }
}
