// for-of, Array.from and destructuring follow a patched iterator protocol (ECMA-262 §7.4.3,
// §7.4.4): a user `next` on a built-in iterator prototype (iter_patched_spec.js has the rest).
// v4: [[[1,2],[[1,2]],[3],["a","b"]],[[10,20],[30],90,[60],[70]],[["1=2"],["a=b"]],[["s1","s2"],["s3"]],["A","B"],[1,2,3],"TypeError",[[1,2],[[3,4]],[5]]]
function err(f) { try { return f() } catch (e) { return e.name } }
function collect(v) { var out = []; for (var x of v) out.push(x); return out }
function proto(v) { return Object.getPrototypeOf(v[Symbol.iterator]()) }
function patchNext(p, f) {
  var orig = p.next;
  p.next = function () { var r = orig.call(this); if (!r.done) r.value = f(r.value); return r };
  return function () { p.next = orig };
}
var before = [collect([1, 2]), collect(new Map([[1, 2]])), collect(new Set([3])), collect("ab")];
var u8 = new Uint8Array([6]);
var undo = patchNext(proto([]), function (v) { return v * 10 });
var arr = [collect([1, 2]), Array.from([3]), (function () { var [a, b] = [4, 5]; return a + b })(),
  collect(u8), (function () { return collect(arguments) })(7)];
undo();
undo = patchNext(proto(new Map()), function (v) { return v.join("=") });
var map = [collect(new Map([[1, 2]])), Array.from(new Map([["a", "b"]]))];
undo();
undo = patchNext(proto(new Set()), function (v) { return "s" + v });
var set = [collect(new Set([1, 2])), Array.from(new Set([3]))];
undo();
undo = patchNext(proto("x"), function (v) { return v.toUpperCase() });
var str = collect("ab");
undo();
var AIP = proto([]), origNext = AIP.next;
Object.defineProperty(AIP, "next", { get: function () { return origNext }, configurable: true });
var viaGetter = collect([1, 2, 3]);
Object.defineProperty(AIP, "next", { value: origNext, writable: true, configurable: true });
delete AIP.next;
var deleted = err(function () { return collect([1]) });
AIP.next = origNext;
var restored = [collect([1, 2]), collect(new Map([[3, 4]])), collect(new Set([5]))];
var result = JSON.stringify([before, arr, map, set, str, viaGetter, deleted, restored]);
