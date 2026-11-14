import QtQuick
Item {
    id: root
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    function d(o, k) { var x = Object.getOwnPropertyDescriptor(o, k); return x ? ("value" in x ? "w=" + x.writable : "acc") + " e=" + x.enumerable + " c=" + x.configurable : "none" }
    Component.onCompleted: {
        t("H1", function() { DOMException.SYNTAX_ERR = 99; return DOMException.SYNTAX_ERR + " " + d(DOMException, "SYNTAX_ERR") })
        t("H2", function() { var o = Array.prototype.push; Array.prototype.push = function() {}; return (Array.prototype.push === o) + " " + d(Array.prototype, "push") })
        t("H3", function() { var o = Map.prototype.get; Map.prototype.get = function() {}; return (Map.prototype.get === o) + " " + d(Map.prototype, "get") })
        t("H4", function() { return [d(Map.prototype, "set"), d(Math, "max"), d(Function.prototype, "connect"), d(Math, "PI"), d(JSON, "parse"), d(Object, "keys"), d(String.prototype, "arg"), d(Number, "MAX_VALUE"), d(Symbol, "iterator"), d(console, "log"), d(Date, "now"), d(SQLException, "DATABASE_ERR")].join(" | ") })
        t("H5", function() { return [d(Object.prototype, "toString"), d(Object.prototype, "toLocaleString"), d(Object.prototype, "valueOf"), d(Object.prototype, "constructor"), d(Array.prototype, "toString"), d(Date.prototype, "valueOf"), d(Array.prototype, "constructor"), d(Error.prototype, "toString"), d(Error.prototype, "name"), d(Error.prototype, "message"), d(TypeError.prototype, "name")].join(" | ") })
        t("H6", function() { Array.prototype.b1x = 1; var r = [].b1x + " " + d(Array.prototype, "b1x"); delete Array.prototype.b1x; return r + " " + [].b1x })
        t("H7", function() { Math.b1y = 2; var r = Math.b1y + " " + d(Math, "b1y"); return r + " " + delete Math.b1y + " " + Math.b1y })
        t("H8", function() { return [Object.isFrozen(Array.prototype), Object.isSealed(Array.prototype), Object.isExtensible(Array.prototype), Object.isFrozen(Math), Object.isExtensible(Math), Object.isExtensible(Object.prototype), Object.isFrozen(DOMException), Object.isExtensible(DOMException), Object.isExtensible(Array), Object.isFrozen(JSON)].join() })
        t("H9", function() { return delete Array.prototype.push + " " + typeof Array.prototype.push + " " + delete Math.max + " " + typeof Math.max + " " + delete DOMException.SYNTAX_ERR + " " + DOMException.SYNTAX_ERR })
        t("H10", function() { "use strict"; try { Array.prototype.push = 1; return "nothrow" } catch (e) { return e.name } })
        t("H11", function() { "use strict"; try { delete Math.max; return "nothrow" } catch (e) { return e.name } })
        t("H12", function() { try { Object.defineProperty(Array.prototype, "push", { value: 1 }); return "nothrow " + typeof Array.prototype.push } catch (e) { return e.name } })
        t("H13", function() { var o = Object.prototype.toString; Object.prototype.toString = function() { return "patched" }; var r = String({}) + " " + d(Object.prototype, "toString"); Object.prototype.toString = o; return r + " " + String({}) })
        t("H14", function() { var o = Array.prototype.toString; Array.prototype.toString = function() { return "ap" }; var r = String([1]); Array.prototype.toString = o; return r + " " + String([1]) })
        t("H15", function() { var o = Date.prototype.valueOf; Date.prototype.valueOf = function() { return 7 }; var r = +new Date(0); Date.prototype.valueOf = o; return r + " " + (+new Date(0)) })
        t("H16", function() { var o = Array.prototype.constructor; Array.prototype.constructor = Object; var r = [].constructor === Object; Array.prototype.constructor = o; return r + " " + ([].constructor === Array) })
        t("H17", function() { var a = []; a.push = function() { return "own" }; return a.push() + " " + (function() { var m = new Map(); m.get = function() { return "own" }; return m.get() })() })
        t("H18", function() { function F() {} F.prototype = Object.create(Array.prototype); F.prototype.push = function() { return "sub" }; return new F().push() })
        t("H19", function() { var o = Object.create(Math); o.max = 5; return o.max + " " + o.hasOwnProperty("max") })
        t("H20", function() { var o = {}; o.toString = 5; o.hasOwnProperty = 6; return o.toString + " " + o.hasOwnProperty })
        t("H21", function() { var e = new Error("m"); e.name = "N"; e.message = "q"; return e.name + e.message + " " + String(e) })
        t("H22", function() { Error.prototype.name = "Z"; TypeError.prototype.message = "tm"; return new Error("x").name + " " + new TypeError().message })
        t("H23", function() { return [d(Qt, "rgba"), d(Qt, "platform"), d(Qt, "LeftButton"), d(console, "warn")].join(" | ") })
        t("H24", function() { Qt.rgba = 1; Qt.b1z = 2; console.log2 = 3; return typeof Qt.rgba + " " + Qt.b1z + " " + console.log2 })
        t("H25", function() { Number.MAX_VALUE = 1; Math.PI = 3; Symbol.iterator = 1; String.fromCharCode = 2; return (Number.MAX_VALUE > 1) + " " + (Math.PI > 3) + " " + typeof Symbol.iterator + " " + typeof String.fromCharCode })
        t("H26", function() { var names = ["Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "Date", "RegExp", "Error", "Map", "Set", "WeakMap", "WeakSet", "Promise", "ArrayBuffer", "DataView", "Uint8Array", "Math", "JSON", "Reflect", "DOMException", "SQLException"]; var bad = []; var n = 0; names.forEach(function(nm) { var C = eval(nm); [C, C.prototype].forEach(function(o, pi) { if (!o) return; Object.getOwnPropertyNames(o).forEach(function(k) { var x = Object.getOwnPropertyDescriptor(o, k); n++; var free = ["toString", "toLocaleString", "valueOf", "constructor"].indexOf(k) >= 0; var locked = "value" in x ? (!x.writable && !x.configurable) : !x.configurable; if (locked === free) bad.push(nm + (pi ? ".prototype." : ".") + k + ":" + ("value" in x ? "w" + x.writable : "acc") + "c" + x.configurable) }) }) }); return n > 300 ? "many " + bad.join(" ") : n + " " + bad.join(" ") })
        t("H27", function() { var bad = []; [Array.prototype, Map.prototype, Symbol.prototype, Promise.prototype, String.prototype].forEach(function(o, i) { Object.getOwnPropertySymbols(o).forEach(function(k) { var x = Object.getOwnPropertyDescriptor(o, k); bad.push(i + String(k) + ("value" in x ? "w" + x.writable : "acc") + "c" + x.configurable) }) }); return bad.join(" ") })
        t("H28", function() { var o = Array.prototype.push; var f = Array.prototype.push; f.b1p = 1; f.length = 9; return f.b1p + " " + f.length + " " + d(f, "length") + " " + d(f, "name") + " " + Object.isExtensible(f) })
        t("H29", function() { Array.prototype[Symbol.iterator] = 1; String.prototype.arg = 2; Function.prototype.connect = 3; return typeof Array.prototype[Symbol.iterator] + typeof "".arg + typeof Function.prototype.connect })
        t("H30", function() { Array.prototype.length = 5; Function.prototype.length = 3; String.prototype.length = 4; return Array.prototype.length + " " + d(Array.prototype, "length") + " " + Function.prototype.length + " " + d(Function.prototype, "length") + " " + String.prototype.length + " " + Function.prototype.name + d(Function.prototype, "name") })
        t("H31", function() { var gp = Object.getPrototypeOf(function*() {}); var ip = Object.getPrototypeOf(Object.getPrototypeOf([].keys())); var ai = Object.getPrototypeOf([].keys()); return d(gp, "prototype") + " | " + d(ai, "next") + " | " + d(ip, Symbol.iterator) + " | " + d(Object.getPrototypeOf(Uint8Array).prototype, "set") + " | " + d(Object.getPrototypeOf(Uint8Array), "from") })
        t("H32", function() { Object.setPrototypeOf(Array.prototype, null); var r = Object.getPrototypeOf(Array.prototype) === Object.prototype; return r })
        t("H33", function() { var r = Reflect.set(Array.prototype, "push", 1) + " " + Reflect.defineProperty(Math, "max", { value: 1 }) + " " + Reflect.deleteProperty(Math, "max") + " " + Reflect.set(Array.prototype, "b1q", 1) + " " + Reflect.deleteProperty(Array.prototype, "b1q"); return r })
        Qt.quit()
    }
}
