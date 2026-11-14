// Species semantics where Qt 6.12's V4 deviates: Array methods make their results with
// ArraySpeciesCreate (ECMA-262 §10.4.2.3; V4 always makes a plain Array), a Promise's custom
// species constructor makes `then`'s promise (§27.2.5.4; V4 throws), and String.prototype.split
// calls the separator's @@split (§22.1.3.23), which RegExp makes with its species (§22.2.6.14).
function thrown(f) { try { f(); return "none" } catch (e) { return e.name } }
class A extends Array {}
var a = A.from([1, 2, 3, 4]);
var arrays = [a.map(function (x) { return x * 2 }), a.filter(function (x) { return x % 2 }),
    a.slice(1), a.splice(0, 1), a.concat([9])].map(function (r) { return (r instanceof A) + ":" + r }).join("|");
var made = [];
function Spec(n) { made.push(n); return new Array(n) }
var arr = [1, 2, 3];
arr.constructor = {};
arr.constructor[Symbol.species] = Spec;
var custom = [String(arr.map(function (x) { return x + 1 })), String(arr.slice(1)), made.join()].join("|");
var plain = [1, 2];
plain.constructor = undefined;
var nulls = [1, 2];
nulls.constructor = {};
nulls.constructor[Symbol.species] = null;
var fallback = [Array.isArray(plain.map(String)), Object.getPrototypeOf(nulls.slice()) === Array.prototype].join();
var bad = [1];
bad.constructor = {};
bad.constructor[Symbol.species] = 5;
var prim = [1];
prim.constructor = 1;
var errors = [thrown(function () { bad.map(String) }), thrown(function () { prim.map(String) })].join();
var calls = 0;
var pr = Promise.resolve(1);
pr.constructor = {};
pr.constructor[Symbol.species] = function (ex) { calls++; return new Promise(ex) };
var promises = [pr.then(function () {}) instanceof Promise, calls];
var fp = Promise.resolve(1);
fp.constructor = {};
fp.constructor[Symbol.species] = function (ex) { calls++; return new Promise(ex) };
promises.push(fp.finally(function () {}) instanceof Promise, calls);
var log = [];
var re = /-/;
re.constructor = {};
re.constructor[Symbol.species] = function (src, flags) { log.push(String(src) + " " + flags); return new RegExp(src, flags) };
var sep = {};
sep[Symbol.split] = function (s, lim) { return "custom " + s + " " + lim };
class R extends RegExp { exec(s) { R.n = (R.n || 0) + 1; return super.exec(s) } }
var splits = [String("a-b-c".split(re)), log.join(), "abc".split(sep, 2), String("a1b2c".split(new R("\\d"))), R.n,
    String("a1b2c".split(/(\d)/, 3)), JSON.stringify("".split(/(?:)/)), "\ud83d\ude00x".split(/(?:)/u).length,
    thrown(function () { var r = /-/; r.constructor = {}; r.constructor[Symbol.species] = 5; "a-b".split(r) })].join("|");
var result = JSON.stringify([arrays, custom, fallback, errors, promises, splits]);
