import QtQuick
Item {
    id: root
    property QtObject q: QtObject { property int a: 1 }
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    function d(o, k) { var x = Object.getOwnPropertyDescriptor(o, k); return x ? ("value" in x ? "w=" + x.writable : "acc") + " e=" + x.enumerable + " c=" + x.configurable : "none" }
    Component.onCompleted: {
        var G = ["Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "Date", "RegExp", "Error", "EvalError", "RangeError", "ReferenceError", "SyntaxError", "TypeError", "URIError", "Map", "Set", "WeakMap", "WeakSet", "Promise", "ArrayBuffer", "SharedArrayBuffer", "DataView", "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "Proxy", "URL", "URLSearchParams", "XMLHttpRequest"]
        var NS = ["Math", "JSON", "Reflect", "Atomics", "Qt", "console", "DOMException", "SQLException", "Easing", "Locale"]
        var FN = ["parseInt", "parseFloat", "isNaN", "isFinite", "escape", "unescape", "eval", "encodeURI", "decodeURI", "encodeURIComponent", "decodeURIComponent", "qsTr", "qsTrId", "print", "gc", "setTimeout"]
        G.forEach(function(n) {
            t("tag " + n, function() { var C = eval(n); var p = C.prototype; return Object.prototype.toString.call(p) + " " + (p === undefined ? "-" : String(p[Symbol.toStringTag]) + " " + d(p, Symbol.toStringTag)) + " | " + Object.prototype.toString.call(C) })
            t("proto " + n, function() { var C = eval(n); var o = C.prototype; var dd = d(C, "prototype"); var th = "ok"; try { C.prototype = {} } catch (e) { th = e.name + ": " + e.message } return dd + " kept=" + (C.prototype === o) + " " + th })
            t("len " + n, function() { var C = eval(n); var l = C.length; var dd = d(C, "length"); C.length = 55; var r = dd + " " + l + "->" + C.length + " del=" + (delete C.length) + " now=" + C.length; return r + " name " + d(C, "name") + " " + C.name })
        })
        NS.forEach(function(n) {
            t("ns " + n, function() { var o = eval(n); return Object.prototype.toString.call(o) + " ext=" + Object.isExtensible(o) + " frozen=" + Object.isFrozen(o) + " sealed=" + Object.isSealed(o) })
        })
        FN.forEach(function(n) {
            t("fn " + n, function() { var f = eval(n); var l = f.length; f.length = 9; return d(f, "length") + " " + l + "->" + f.length + " name " + d(f, "name") + " " + f.name + " proto=" + d(f, "prototype") })
        })
        t("TA1", function() { var T = Object.getPrototypeOf(Uint8Array); var g = Object.getOwnPropertyDescriptor(T.prototype, Symbol.toStringTag); return typeof g.get + " " + g.set + " e=" + g.enumerable + " c=" + g.configurable + " " + g.get.call(new Uint8Array(1)) + " " + g.get.call({}) + " " + g.get.name + " " + Object.prototype.toString.call(T.prototype) + " " + T.name + " " + d(T, "prototype") })
        t("TA2", function() { return ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array"].map(function(n) { var C = eval(n); return Object.prototype.toString.call(new C(1)) + Object.prototype.toString.call(C.prototype) + C.prototype[Symbol.toStringTag] + C.prototype.hasOwnProperty(Symbol.toStringTag) }).join(" ") })
        t("TG1", function() { return [new Map(), new Set(), new WeakMap(), new WeakSet(), new ArrayBuffer(1), new DataView(new ArrayBuffer(1)), Promise.resolve(), Symbol(), new String(""), new Number(1), new Boolean(true), new Date(0), /a/, [], function() {}, new Error(), Math, JSON, Reflect, null, undefined, 1, "s", true, (function() { return arguments })()].map(function(v) { return Object.prototype.toString.call(v) }).join("") })
        t("TG2", function() { return [Map, Set, WeakMap, WeakSet, ArrayBuffer, DataView, Promise, Symbol].map(function(C) { return d(C.prototype, Symbol.toStringTag) + ":" + String(C.prototype[Symbol.toStringTag]) }).join(" | ") })
        t("TG3", function() { return [String, Number, Boolean, Date, RegExp, Array, Function, Error, Object].map(function(C) { return d(C.prototype, Symbol.toStringTag) + ":" + String(C.prototype[Symbol.toStringTag]) }).join(" | ") })
        t("TG4", function() { var m = new Map(); m[Symbol.toStringTag] = "X"; var o = {}; o[Symbol.toStringTag] = "Y"; var p = Object.create(Map.prototype); return Object.prototype.toString.call(m) + Object.prototype.toString.call(o) + Object.prototype.toString.call(p) + Object.prototype.toString.call(Object.create(Set.prototype)) })
        t("TG5", function() { return String(String.prototype.valueOf()) + "|" + Number.prototype.valueOf() + "|" + Boolean.prototype.valueOf() + "|" + String.prototype.length + "|" + typeof String.prototype + typeof Number.prototype + "|" + (String.prototype + "x") + (Number.prototype + 1) + (Boolean.prototype + "") })
        t("TG6", function() { return [Object.prototype.toString.call(Array.prototype), Array.isArray(Array.prototype), Array.prototype.length, Object.prototype.toString.call(Function.prototype), typeof Function.prototype, Object.prototype.toString.call(RegExp.prototype), Object.prototype.toString.call(Date.prototype), Object.prototype.toString.call(Error.prototype), Object.prototype.toString.call(Symbol.prototype), Object.prototype.toString.call(Promise.prototype)].join(" ") })
        t("TG7", function() { return [Object.prototype.toString.call([].keys()), Object.prototype.toString.call(new Map().keys()), Object.prototype.toString.call(new Set().values()), Object.prototype.toString.call(""[Symbol.iterator]()), Object.prototype.toString.call((function*() {})()), Object.prototype.toString.call(function*() {})].join(" ") })
        t("RX1", function() { var r = /a/g; var dd = d(r, "lastIndex"); var del = delete r.lastIndex; r.lastIndex = 3; return dd + " " + del + " " + r.lastIndex + " " + Object.getOwnPropertyNames(r).join() })
        t("RX2", function() { var r = new RegExp("a"); var th = "ok"; try { Object.defineProperty(r, "lastIndex", { configurable: true }) } catch (e) { th = e.name } return d(r, "lastIndex") + " " + th })
        t("RX3", function() { "use strict"; var r = /a/; try { delete r.lastIndex; return "nothrow" } catch (e) { return e.name + ": " + e.message } })
        t("RX4", function() { var r = /a/; Object.defineProperty(r, "lastIndex", { writable: false }); return d(r, "lastIndex") + " " + Object.isFrozen(Object.freeze(/b/)) })
        t("PX1", function() { return d(Proxy, "length") + " " + Proxy.length + " " + d(Proxy, "name") + " " + Proxy.name + " " + d(Proxy, "revocable") + " " + Proxy.revocable.length + " " + d(Proxy.revocable, "length") + " " + Object.getOwnPropertyNames(Proxy).join() })
        t("PX2", function() { "use strict"; try { Proxy.length = 5; return "nothrow " + Proxy.length } catch (e) { return e.name + ": " + e.message } })
        t("PX3", function() { var r = Proxy.revocable({}, {}); return d(r.revoke, "length") + " " + r.revoke.length + " " + delete Proxy.length + " " + Proxy.length })
        Qt.quit()
    }
}
