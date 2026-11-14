// Spec semantics of a patched iterator protocol where Qt 6.12's V4 deviates (as
// iter_patched.js): GetIterator reads `next` once (§7.4.3; V4 reads it at every step) and
// calls the `@@iterator` an Array or Map has, replaced, own or none (§7.4.3 step 2; V4 keeps
// its own iteration), Array.from takes an Array without one as array-like (§23.1.2.1 step 6)
// and a string through its patched iterator, and URLSearchParams has the WebIDL pair iterator
// (V4 has no `@@iterator`).
function err(f) { try { return f() } catch (e) { return e.name } }
function collect(v) { var out = []; for (var x of v) out.push(x); return out }
var AIP = Object.getPrototypeOf([][Symbol.iterator]()), origNext = AIP.next, calls = 0;
Object.defineProperty(AIP, "next", { get: function () { calls++; return origNext }, configurable: true });
var viaGetter = [collect([1, 2, 3]), calls];
AIP.next = origNext;
var saved = Array.prototype[Symbol.iterator];
delete Array.prototype[Symbol.iterator];
var missing = [err(function () { return collect([1]) }), Array.from([1, 2]),
  (function () { return collect(arguments) })(3)];
Array.prototype[Symbol.iterator] = function () {
  var i = 0, a = this;
  return { next: function () { return i < a.length ? { value: -a[i++], done: false } : { done: true } } };
};
var replaced = collect([1, 2]);
Array.prototype[Symbol.iterator] = saved;
var m2 = new Map([[1, 2]]);
m2[Symbol.iterator] = function* () { yield "own" };
var instance = [collect(m2), collect(new Map([[1, 2]])), collect(new Set([1]))];
var SIP = Object.getPrototypeOf(""[Symbol.iterator]()), strNext = SIP.next;
SIP.next = function () { var r = strNext.call(this); if (!r.done) r.value = r.value.toUpperCase(); return r };
var str = Array.from("ab");
SIP.next = strNext;
var params = new URLSearchParams("a=1&b=2");
var before = collect(params);
var proto = Object.getPrototypeOf(params[Symbol.iterator]()), orig = proto.next;
proto.next = function () { var r = orig.call(this); if (!r.done) r.value = r.value.join(":"); return r };
var patched = [collect(params), Array.from(params)];
proto.next = orig;
var savedParams = URLSearchParams.prototype[Symbol.iterator];
URLSearchParams.prototype[Symbol.iterator] = function* () { yield "own" };
var replaced = collect(params);
URLSearchParams.prototype[Symbol.iterator] = savedParams;
var result = JSON.stringify([viaGetter, missing, replaced, instance, str, before, patched, replaced, collect(params)]);
