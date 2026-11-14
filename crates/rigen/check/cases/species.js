// v4: ["true,true|true,true|true,true|true,true|true,true|true,true|true,true|true,true","true,true,true,2,2,b,2,8,true","true:2,3,4,5|true:2,3,4|true:3,4|true:2,3","[object Int16Array]:-5,-6,-7|[object Int16Array]:6,7","true,4,TypeError",[true,true,"TypeError"]]
// @@species (ECMA-262 §7.3.22 SpeciesConstructor, §23.2.4.1 TypedArraySpeciesCreate): the
// built-in getters return their receiver, and subclasses and custom species constructors make
// the results of the typed-array, ArrayBuffer and Promise methods; `Array.from` / `Array.of`
// fill a new `this` (§23.1.2.1, §23.1.2.3).
var ctors = [Array, ArrayBuffer, SharedArrayBuffer, Map, Set, Promise, RegExp, Object.getPrototypeOf(Int8Array)];
var getters = ctors.map(function (c) {
    return [c[Symbol.species] === c, Object.getOwnPropertySymbols(c).indexOf(Symbol.species) >= 0].join();
}).join("|");
function thrown(f) { try { f(); return "none" } catch (e) { return e.name } }
class T extends Uint8Array {}
var t = new T([1, 2, 3, 4]);
var typed = [t.map(function (x) { return x + 1 }), t.filter(function (x) { return x > 1 }), t.slice(2),
    t.subarray(1, 3)].map(function (r) { return (r instanceof T) + ":" + r }).join("|");
var u = new Uint8Array([5, 6, 7]);
u.constructor = {};
u.constructor[Symbol.species] = Int16Array;
var typedCustom = [u.map(function (x) { return -x }), u.slice(1)].map(function (r) {
    return Object.prototype.toString.call(r) + ":" + r }).join("|");
class B extends ArrayBuffer {}
var bs = new B(8).slice(2, 6);
var buf = new ArrayBuffer(4);
buf.constructor = {};
buf.constructor[Symbol.species] = function () { return buf };
var buffers = [bs instanceof B, bs.byteLength, thrown(function () { buf.slice(0) })].join();
class P extends Promise {}
var promises = [P.resolve(1).then() instanceof P, P.resolve(1).catch(function () {}) instanceof P];
var bp = Promise.resolve(1);
bp.constructor = 1;
promises.push(thrown(function () { bp.then() }));
class L extends Array {}
function C(n) { this.n = n }
var x = Array.from.call(C, {length: 2, 0: "a", 1: "b"}), y = Array.of.call(C, 7, 8);
var from = [L.from([1]) instanceof L, L.of(1) instanceof L, x instanceof C, x.n, x.length, x[1], y.n, y[1],
    Array.isArray(Array.from.call({}, [1]))].join();
var result = JSON.stringify([getters, from, typed, typedCustom, buffers, promises]);
