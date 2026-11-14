// Round-seven runtime findings where Qt 6.12's V4 deviates from ECMA-262 and the runtime
// follows the spec: V4's Map/Set constructors ignore the current `set`/`add` and step the
// whole iterator first (§24.1.1.2), it renames a named function expression under a symbol key
// (§13.2.5.5 NamedEvaluation applies to anonymous ones only), leaves a class's `prototype`
// configurable (§15.7.14) and puts a re-created property back in its old place
// (§10.1.11.1). Each case runs under `t`, which records its result or what it threw.
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name);
    }
}
function d(o, k) { return JSON.stringify(Object.getOwnPropertyDescriptor(o, k)) }
t("map patched set", function() { var orig = Map.prototype.set, log = []; Map.prototype.set = function(k, v) { log.push(k); return orig.call(this, k, v) }; try { var m = new Map([[1, 2], [3, 4]]); return log.join() + "," + m.size } finally { Map.prototype.set = orig } })
t("set patched add", function() { var orig = Set.prototype.add, log = []; Set.prototype.add = function(v) { log.push(v); return orig.call(this, v) }; try { var s = new Set([5, 6]); return log.join() + "," + s.size } finally { Set.prototype.add = orig } })
t("set add not callable", function() { var orig = Set.prototype.add; Set.prototype.add = 1; try { new Set([1]); return "no throw" } finally { Set.prototype.add = orig } })
t("set add throws closes", function() { var orig = Set.prototype.add, closed = 0, it = {}; var n = 0; it[Symbol.iterator] = function() { return {next: function() { return n++ < 3 ? {value: 1, done: false} : {done: true} }, return: function() { closed++; return {} }} }; Set.prototype.add = function() { throw new RangeError("x") }; try { new Set(it) } catch (e) { return e.name + closed } finally { Set.prototype.add = orig } })
t("set steps lazily", function() { var log = [], it = {}, n = 0; it[Symbol.iterator] = function() { return {next: function() { log.push("next"); return n < 2 ? {value: n++, done: false} : {done: true} }} }; var orig = Set.prototype.add; Set.prototype.add = function(v) { log.push("add" + v); return orig.call(this, v) }; try { new Set(it); return log.join() } finally { Set.prototype.add = orig } })
t("sym named fn keeps name", function() { var s = Symbol("x"); return ({[s]: function own() {}})[s].name })
t("class prototype", function() { class C {} return d(C, "prototype").replace(/"value":\{\},/, "") })
t("class prototype write", function() { class C {} var p = C.prototype; C.prototype = 1; return C.prototype === p })
t("class prototype strict write", function() { "use strict"; class C {} C.prototype = 1 })
t("order delete recreate", function() { var o = {a: 1, b: 2}; Object.defineProperty(o, "c", {value: 3, configurable: true}); delete o.a; o.a = 4; return Object.getOwnPropertyNames(o) })
var result = JSON.stringify(r);
