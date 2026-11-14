import QtQuick
Item {
    id: root
    property color c: "#336699"
    property QtObject q: QtObject { property int a: 1 }
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    function d(o, k) { var x = Object.getOwnPropertyDescriptor(o, k); return x ? "w=" + x.writable + " e=" + x.enumerable + " c=" + x.configurable : "none" }
    Component.onCompleted: {
        t("A1", function() { return [Map, Set, ArrayBuffer, DataView].map(function(C) { return String(C.prototype[Symbol.toStringTag]) }).join() })
        t("A2", function() { return Object.prototype.toString.call(Map.prototype) + Object.prototype.toString.call(Set.prototype) })
        t("A4", function() { return Object.prototype.toString.call(String.prototype) + Object.prototype.toString.call(Number.prototype) + Object.prototype.toString.call(Boolean.prototype) })
        t("A5", function() { return typeof Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype), Symbol.toStringTag) })
        t("B1", function() { return ["Symbol", "Date", "RegExp", "Map", "Set", "WeakMap", "Promise", "ArrayBuffer", "DataView", "Uint8Array"].map(function(n) { return n + ":" + Object.getOwnPropertyDescriptor(eval(n), "prototype").writable }).join(" ") })
        t("B2", function() { var o = Map.prototype; Map.prototype = {}; var r = Map.prototype === o; return r })
        t("B3", function() { return d(Proxy, "length") })
        t("B4", function() { Proxy.length = 5; return Proxy.length })
        t("B5", function() { return d(/a/, "lastIndex") })
        t("B6", function() { var r = /a/g; return delete r.lastIndex })
        t("C1", function() { return q.hasOwnProperty("a") + "," + q.hasOwnProperty("zz") })
        t("C2", function() { return root.c.hasOwnProperty("r") })
        t("C3", function() { return "r" in root.c })
        t("C4", function() { return Object.keys(root.c).join() })
        t("C5", function() { return String(root.c.lighter()) + " " + String(root.c.darker()) })
        t("C6", function() { return String(root.c.alpha(0.5)) })
        t("C7", function() { return String(root.c.tint("#80ff0000")) })
        t("C8", function() { return root.c.rgba })
        t("C9", function() { return typeof root.c.lighter })
        t("C10", function() { return Object.freeze(q) === q })
        t("D1", function() { root.c = "light blue"; return String(root.c) + " " + root.c.valid })
        t("D2", function() { root.c = " #fff"; return String(root.c) + " " + root.c.valid })
        t("E1", function() { return /\w/i.test("\u017f") + "," + /[a-z]/i.test("\u212a") + "," + /I/i.test("\u0131") })
        t("E2", function() { return /\u{1f600}/.test("u{1f600}") })
        t("E3", function() { return /[\ud83d\ude00]/.test("\ud83d") })
        t("F1", function() { return Function.prototype.toString.call(function named(a) { return a }) })
        t("F2", function() { root.widthChanged.connect(null, function() {}); return "ok" })
        t("G1", function() { return "%1".arg(Math.round(1234567.8)) + " " + "%1".arg(parseInt("1234567")) + " " + "%1".arg(4294967295) })
        t("G2", function() { return "%1".arg(0.00005) + " " + "%1".arg(100000.5) })
        t("G3", function() { return "%1".arg(false) + " " + "%09 %9".arg("x") + " " + "%001".arg("x") })
        t("G4", function() { return "%1".arg(Symbol()) })
        t("H1", function() { DOMException.SYNTAX_ERR = 99; return DOMException.SYNTAX_ERR })
        Qt.quit()
    }
}
