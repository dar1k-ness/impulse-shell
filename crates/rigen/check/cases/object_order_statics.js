// Round-seven runtime findings where Qt 6.12's V4 agrees with ECMA-262: IteratorClose in the
// Map constructor (§24.1.1.2), SetFunctionName for symbol keys (§10.2.9), the attributes of
// well-known symbols (§20.4.2), relational comparison of symbols (§7.2.13), ToObject in
// Object.keys/values/entries (§20.1.2), OrdinaryOwnPropertyKeys order (§10.1.11.1) and [[Get]]
// on a primitive base through accessors (§6.2.5.5). Each case runs under `t`, which records
// its result or what it threw.
// v4: ["map closes iterator => TypeError1","sym fn name => [nm]||","sym arrow name => [d]","sym accessor names => get [g]|set [g]","well-known attrs => {\"writable\":false,\"enumerable\":false,\"configurable\":false}{\"writable\":false,\"enumerable\":false,\"configurable\":false}","well-known write => true","well-known strict write THROW TypeError","sym lt THROW TypeError","sym gt str THROW TypeError","sym le obj THROW TypeError","fn prototype writable => 1","keys null THROW TypeError","values undefined THROW TypeError","entries null THROW TypeError","keys primitive => 0,1|0|[[\"0\",\"x\"]]","order nonenum => a,z,b","order array length => 0,1,length,x","order reenable => a,b|{\"a\":1,\"b\":2}","order accessor => a,g,b|a,g,b","order indices => 1,2,b,a","prim getter => object3","prim getter strict => number","prim getter bool => true","prim object proto getter => stringnumber"]
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name);
    }
}
function d(o, k) { return JSON.stringify(Object.getOwnPropertyDescriptor(o, k)) }
t("map closes iterator", function() { var closed = 0, it = {}; var n = 0; it[Symbol.iterator] = function() { return {next: function() { return n++ < 3 ? {value: 1, done: false} : {done: true} }, return: function() { closed++; return {} }} }; try { new Map(it) } catch (e) { return e.name + closed } })
t("sym fn name", function() { var s = Symbol("nm"), e = Symbol(); var o = {[s]: function() {}, [e]: () => 1}; return o[s].name + "|" + o[e].name + "|" })
t("sym arrow name", function() { var s = Symbol("d"); return ({[s]: () => 1})[s].name })
t("sym accessor names", function() { var g = Symbol("g"); var o = {get [g]() { return 1 }, set [g](v) {}}; var ds = Object.getOwnPropertyDescriptor(o, g); return ds.get.name + "|" + ds.set.name })
t("well-known attrs", function() { return d(Symbol, "iterator") + d(Symbol, "toPrimitive") })
t("well-known write", function() { var i = Symbol.iterator; Symbol.iterator = 1; delete Symbol.iterator; return Symbol.iterator === i })
t("well-known strict write", function() { "use strict"; Symbol.iterator = 1 })
t("sym lt", function() { return Symbol() < 1 })
t("sym gt str", function() { return "a" > Symbol("a") })
t("sym le obj", function() { return {valueOf: function() { return Symbol() }} <= 1 })
t("fn prototype writable", function() { function F() {} F.prototype = 1; return F.prototype })
t("keys null", function() { return Object.keys(null) })
t("values undefined", function() { return Object.values(undefined) })
t("entries null", function() { return Object.entries(null) })
t("keys primitive", function() { return Object.keys("ab") + "|" + Object.values(5).length + "|" + JSON.stringify(Object.entries("x")) })
t("order nonenum", function() { var o = {a: 1}; Object.defineProperty(o, "z", {value: 1}); o.b = 2; return Object.getOwnPropertyNames(o) })
t("order array length", function() { var a = [1, 2]; a.x = 1; return Reflect.ownKeys(a) })
t("order reenable", function() { var o = {a: 1, b: 2}; Object.defineProperty(o, "a", {enumerable: false}); Object.defineProperty(o, "a", {enumerable: true}); return Object.keys(o) + "|" + JSON.stringify(o) })
t("order accessor", function() { var o = {a: 1}; Object.defineProperty(o, "g", {get: function() { return 1 }, configurable: true}); o.b = 2; Object.defineProperty(o, "g", {enumerable: true}); return Object.getOwnPropertyNames(o) + "|" + Object.keys(o) })
t("order indices", function() { var o = {b: 1}; Object.defineProperty(o, "2", {value: 1}); o[1] = 1; o.a = 1; return Reflect.ownKeys(o) })
t("prim getter", function() { Object.defineProperty(String.prototype, "gt", {get: function() { return typeof this + this.length }, configurable: true}); try { return "xyz".gt } finally { delete String.prototype.gt } })
t("prim getter strict", function() { Object.defineProperty(Number.prototype, "gn", {get: function() { "use strict"; return typeof this }, configurable: true}); try { return (5).gn } finally { delete Number.prototype.gn } })
t("prim getter bool", function() { Object.defineProperty(Boolean.prototype, "gb", {get: function() { return this.valueOf() }, configurable: true}); try { return true.gb } finally { delete Boolean.prototype.gb } })
t("prim object proto getter", function() { Object.defineProperty(Object.prototype, "go", {get: function() { "use strict"; return typeof this }, configurable: true}); try { return "s".go + (1).go } finally { delete Object.prototype.go } })
var result = JSON.stringify(r);
