import QtQuick
Item {
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    function d(o, k) { var x = Object.getOwnPropertyDescriptor(o, k); return x ? ("value" in x ? "w=" + x.writable : "acc") + " e=" + x.enumerable + " c=" + x.configurable : "none" }
    Component.onCompleted: {
        t("T1", function() { var a = [1]; a.join = function () { return 'J' }; return String(a) })
        t("T2", function() { var a = [1]; Object.defineProperty(a, 'join', { value: function () { return 'J' } }); return String(a) })
        t("T3", function() { var r = /a/; r.exec = function () { return null }; return r.test('a') })
        t("T4", function() { var r = /a/; Object.defineProperty(r, 'exec', { value: function () { return null } }); return r.test('a') })
        t("T5", function() { var P = Object.getPrototypeOf([][Symbol.iterator]()); var n = P.next; P.next = function() { return { done: true } }; return (P.next === n) + " " + d(P, "next") + " " + [...[1,2]].length })
        t("T6", function() { var P = Object.getPrototypeOf(new Map()[Symbol.iterator]()); var n = P.next; P.next = function() { return { done: true } }; return (P.next === n) + " " + d(P, "next") + " " + [...new Map([[1,2]])].length })
        t("T7", function() { var P = Object.getPrototypeOf(new Set()[Symbol.iterator]()); return d(P, "next") + " " + delete P.next + " " + typeof P.next })
        t("T8", function() { var P = Object.getPrototypeOf(''[Symbol.iterator]()); return d(P, "next") + " " + d(P, Symbol.toStringTag) })
        t("T9", function() { var P = Object.getPrototypeOf([][Symbol.iterator]()); Object.defineProperty(P, 'next', { get: function () { return function () { return { done: true } } } }); return [...[1,2]].length })
        t("T10", function() { var P = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())); return d(P, Symbol.iterator) + " " + Object.getOwnPropertyNames(P) })
        t("T11", function() { var o = Array.prototype[Symbol.iterator]; Array.prototype[Symbol.iterator] = function () { return [].values.call([]) }; return (Array.prototype[Symbol.iterator] === o) + " " + [...[1,2]].length })
        t("T12", function() { var G = Object.getPrototypeOf(function*(){}); var GP = G.prototype; return d(G, "prototype") + " " + d(GP, "next") + " " + d(GP, "return") + " " + d(Object.getPrototypeOf(G), "prototype") })
        t("T13", function() { var TA = Object.getPrototypeOf(Uint8Array); return d(TA, "from") + " " + d(TA.prototype, "set") + " " + d(TA.prototype, "length") + " " + d(TA, "prototype") + " " + d(Uint8Array, "BYTES_PER_ELEMENT") })
        t("T14", function() { function F() {} F.prototype = Object.create(Map.prototype); F.prototype.set = function (k, v) { return 1 }; return F.prototype.hasOwnProperty("set") })
        t("T15", function() { class M extends Map { set(k, v) { return super.set(k, v + 1) } } return new M([[1, 2]]).get(1) })
        t("T16", function() { var o = { push: 1, hasOwnProperty: 2 }; return o.push + o.hasOwnProperty })
        t("T17", function() { var a = []; a.length = 3; var e = new Error("x"); e.stack = 1; return a.length })
        t("T18", function() { var p = Promise.resolve(1); p.then = 5; return typeof p.then + " " + d(Promise.prototype, "then") + " " + d(Promise, "resolve") })
        t("T19", function() { return d(globalThis, "Array") + " " + d(globalThis, "Math") + " " + d(globalThis, "parseInt") + " " + d(globalThis, "undefined") + " " + d(globalThis, "console") + " " + d(globalThis, "Qt") + " " + d(globalThis, "globalThis")})
        t("T20", function() { var P = Object.getPrototypeOf(/a/[Symbol.matchAll]("a")); return d(P, "next") })
        t("T21", function() { var e = Object.getOwnPropertyDescriptor(Error.prototype, "stack"); var m = Object.getOwnPropertyDescriptor(Map.prototype, "size"); return (e ? e.configurable : "none") + " " + m.configurable + " " + Object.getOwnPropertyDescriptor(RegExp.prototype, "flags").configurable + " " + Object.getOwnPropertyDescriptor(RegExp, Symbol.species).configurable })
    }
}
